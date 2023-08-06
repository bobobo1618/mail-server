/*
 * Copyright (c) 2020-2022, Stalwart Labs Ltd.
 *
 * This file is part of Stalwart Mail Server.
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of
 * the License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 * in the LICENSE file at the top-level directory of this distribution.
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * You can be released from the requirements of the AGPLv3 license by
 * purchasing a commercial license. Please contact licensing@stalw.art
 * for more details.
*/

use imap::op::list::matches_pattern;
use imap_proto::ResponseType;

use super::{AssertResult, ImapConnection, Type};

pub async fn test(mut imap: &mut ImapConnection, mut imap_check: &mut ImapConnection) {
    // Create third connection for testing
    let mut other_conn = ImapConnection::connect(b"_z ").await;
    other_conn
        .send("AUTHENTICATE PLAIN {32+}\r\nAGpkb2VAZXhhbXBsZS5jb20Ac2VjcmV0")
        .await;
    other_conn.assert_read(Type::Tagged, ResponseType::Ok).await;

    // List folders
    imap.send("LIST \"\" \"*\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders(
            [
                ("All Mail", ["NoInferiors"]),
                ("INBOX", [""]),
                ("Deleted Items", [""]),
            ],
            true,
        );
    
    // Ensure INBOX is case-insensitive
    imap.send("SELECT \"INBOX\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("SELECT \"Inbox\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("SELECT \"inbox\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Create folders
    imap.send("CREATE \"Tofu\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("CREATE \"Fruit\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("CREATE \"Fruit/Apple\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("CREATE \"Fruit/Apple/Green\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Select folder from another connection
    other_conn.send("SELECT \"Tofu\"").await;
    other_conn.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Make sure folders are visible
    for imap in [&mut imap, &mut imap_check] {
        imap.send("LIST \"\" \"*\"").await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_folders(
                [
                    ("All Mail", ["NoInferiors"]),
                    ("INBOX", [""]),
                    ("Deleted Items", [""]),
                    ("Fruit", [""]),
                    ("Fruit/Apple", [""]),
                    ("Fruit/Apple/Green", [""]),
                    ("Tofu", [""]),
                ],
                true,
            );
    }

    // Folders under All Mail should not be allowed
    imap.send("CREATE \"All Mail/Untitled\"").await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;

    // Special use folders that already exist should not be allowed
    imap.send("CREATE \"Second trash\" (USE (\\Trash))").await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;

    // Enable IMAP4rev2
    imap.send("ENABLE IMAP4rev2").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;

    // Create missing parent folders
    imap.send("CREATE \"/Vegetable/Broccoli\" (USE (\\Important))")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_contains("[MAILBOXID (");

    imap.send("CREATE \" Cars/Electric /4 doors/ Red/\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    for imap in [&mut imap, &mut imap_check] {
        imap.send("LIST \"\" \"*\" RETURN (CHILDREN SPECIAL-USE)")
            .await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_folders(
                [
                    ("All Mail", ["NoInferiors", "All"]),
                    ("INBOX", ["HasNoChildren", ""]),
                    ("Deleted Items", ["HasNoChildren", "Trash"]),
                    ("Cars/Electric/4 doors/Red", ["HasNoChildren", ""]),
                    ("Cars/Electric/4 doors", ["HasChildren", ""]),
                    ("Cars/Electric", ["HasChildren", ""]),
                    ("Cars", ["HasChildren", ""]),
                    ("Fruit", ["HasChildren", ""]),
                    ("Fruit/Apple", ["HasChildren", ""]),
                    ("Fruit/Apple/Green", ["HasNoChildren", ""]),
                    ("Vegetable", ["HasChildren", ""]),
                    ("Vegetable/Broccoli", ["HasNoChildren", "\\Important"]),
                    ("Tofu", ["HasNoChildren", ""]),
                ],
                true,
            );
    }

    // Rename folders
    imap.send("RENAME \"Fruit/Apple/Green\" \"Fruit/Apple/Red\"")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("RENAME \"Cars\" \"Vehicles\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("RENAME \"Vegetable/Broccoli\" \"Veggies/Green/Broccoli\"")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("RENAME \"Tofu\" \"INBOX\"").await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;
    imap.send("RENAME \"Tofu\" \"INBOX/Tofu\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("RENAME \"Deleted Items\" \"Recycle Bin\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    for imap in [&mut imap, &mut imap_check] {
        imap.send("LIST \"\" \"*\" RETURN (CHILDREN SPECIAL-USE)")
            .await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_folders(
                [
                    ("All Mail", ["NoInferiors", "All"]),
                    ("INBOX", ["HasChildren", ""]),
                    ("INBOX/Tofu", ["HasNoChildren", ""]),
                    ("Recycle Bin", ["HasNoChildren", "Trash"]),
                    ("Vehicles/Electric/4 doors/Red", ["HasNoChildren", ""]),
                    ("Vehicles/Electric/4 doors", ["HasChildren", ""]),
                    ("Vehicles/Electric", ["HasChildren", ""]),
                    ("Vehicles", ["HasChildren", ""]),
                    ("Fruit", ["HasChildren", ""]),
                    ("Fruit/Apple", ["HasChildren", ""]),
                    ("Fruit/Apple/Red", ["HasNoChildren", ""]),
                    ("Vegetable", ["HasNoChildren", ""]),
                    ("Veggies", ["HasChildren", ""]),
                    ("Veggies/Green", ["HasChildren", ""]),
                    ("Veggies/Green/Broccoli", ["HasNoChildren", ""]),
                ],
                true,
            );
    }

    // Delete folders
    imap.send("DELETE \"INBOX/Tofu\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("DELETE \"Vegetable\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("DELETE \"All Mail\"").await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;
    imap.send("DELETE \"Vehicles\"").await;
    imap.assert_read(Type::Tagged, ResponseType::No).await;
    for imap in [&mut imap, &mut imap_check] {
        imap.send("LIST \"\" \"*\" RETURN (CHILDREN SPECIAL-USE)")
            .await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_folders(
                [
                    ("All Mail", ["NoInferiors", "All"]),
                    ("INBOX", ["HasNoChildren", ""]),
                    ("Recycle Bin", ["HasNoChildren", "Trash"]),
                    ("Vehicles/Electric/4 doors/Red", ["HasNoChildren", ""]),
                    ("Vehicles/Electric/4 doors", ["HasChildren", ""]),
                    ("Vehicles/Electric", ["HasChildren", ""]),
                    ("Vehicles", ["HasChildren", ""]),
                    ("Fruit", ["HasChildren", ""]),
                    ("Fruit/Apple", ["HasChildren", ""]),
                    ("Fruit/Apple/Red", ["HasNoChildren", ""]),
                    ("Veggies", ["HasChildren", ""]),
                    ("Veggies/Green", ["HasChildren", ""]),
                    ("Veggies/Green/Broccoli", ["HasNoChildren", ""]),
                ],
                true,
            );
    }

    // Subscribe
    imap.send("SUBSCRIBE \"INBOX\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    imap.send("SUBSCRIBE \"Vehicles/Electric/4 doors/Red\"")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    for imap in [&mut imap, &mut imap_check] {
        imap.send("LIST \"\" \"*\" RETURN (SUBSCRIBED SPECIAL-USE)")
            .await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_folders(
                [
                    ("All Mail", ["NoInferiors", "All"]),
                    ("INBOX", ["Subscribed", ""]),
                    ("Recycle Bin", ["", "Trash"]),
                    ("Vehicles/Electric/4 doors/Red", ["Subscribed", ""]),
                    ("Vehicles/Electric/4 doors", ["", ""]),
                    ("Vehicles/Electric", ["", ""]),
                    ("Vehicles", ["", ""]),
                    ("Fruit", ["", ""]),
                    ("Fruit/Apple", ["", ""]),
                    ("Fruit/Apple/Red", ["", ""]),
                    ("Veggies", ["", ""]),
                    ("Veggies/Green", ["", ""]),
                    ("Veggies/Green/Broccoli", ["", ""]),
                ],
                true,
            );
    }

    // Filter by subscribed including children
    imap.send("LIST (SUBSCRIBED) \"\" \"*\" RETURN (CHILDREN)")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders(
            [
                ("INBOX", ["Subscribed", "HasNoChildren"]),
                (
                    "Vehicles/Electric/4 doors/Red",
                    ["Subscribed", "HasNoChildren"],
                ),
            ],
            true,
        );

    // Recursive match including children
    imap.send("LIST (SUBSCRIBED RECURSIVEMATCH) \"\" \"*\" RETURN (CHILDREN)")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders(
            [
                ("INBOX", ["Subscribed", "HasNoChildren"]),
                (
                    "Vehicles/Electric/4 doors/Red",
                    ["Subscribed", "HasNoChildren"],
                ),
                (
                    "Vehicles/Electric/4 doors",
                    ["\"CHILDINFO\" (\"SUBSCRIBED\")", "HasChildren"],
                ),
                (
                    "Vehicles/Electric",
                    ["\"CHILDINFO\" (\"SUBSCRIBED\")", "HasChildren"],
                ),
                (
                    "Vehicles",
                    ["\"CHILDINFO\" (\"SUBSCRIBED\")", "HasChildren"],
                ),
            ],
            true,
        );

    // Imap4rev1 LSUB
    imap.send("LSUB \"\" \"*\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders(
            [("INBOX", [""]), ("Vehicles/Electric/4 doors/Red", [""])],
            true,
        );

    // Unsubscribe
    imap.send("UNSUBSCRIBE \"Vehicles/Electric/4 doors/Red\"")
        .await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
    for imap in [&mut imap, &mut imap_check] {
        imap.send("LIST (SUBSCRIBED RECURSIVEMATCH) \"\" \"*\" RETURN (CHILDREN)")
            .await;
        imap.assert_read(Type::Tagged, ResponseType::Ok)
            .await
            .assert_folders([("INBOX", ["Subscribed", "HasNoChildren"])], true);
    }

    // LIST Filters
    imap.send("LIST \"\" \"%\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders(
            [
                ("All Mail", [""]),
                ("INBOX", [""]),
                ("Recycle Bin", [""]),
                ("Vehicles", [""]),
                ("Fruit", [""]),
                ("Veggies", [""]),
            ],
            true,
        );

    imap.send("LIST \"\" \"*/Red\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders(
            [
                ("Vehicles/Electric/4 doors/Red", [""]),
                ("Fruit/Apple/Red", [""]),
            ],
            true,
        );

    imap.send("LIST \"\" \"Fruit/*\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders([("Fruit/Apple/Red", [""]), ("Fruit/Apple", [""])], true);

    imap.send("LIST \"\" \"Fruit/%\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok)
        .await
        .assert_folders([("Fruit/Apple", [""])], true);

    // Restore Trash folder's original name
    imap.send("RENAME \"Recycle Bin\" \"Deleted Items\"").await;
    imap.assert_read(Type::Tagged, ResponseType::Ok).await;
}

#[test]
fn mailbox_matches_pattern() {
    let mailboxes = [
        "imaptest",
        "imaptest/test",
        "imaptest/test2",
        "imaptest/test3",
        "imaptest/test3/test4",
        "imaptest/test3/test4/test5",
        "foobar/test",
        "foobar/test/test",
        "foobar/test1/test1",
    ];

    for (pattern, expected_match) in [
        (
            "imaptest/%",
            vec!["imaptest/test", "imaptest/test2", "imaptest/test3"],
        ),
        ("imaptest/%/%", vec!["imaptest/test3/test4"]),
        (
            "imaptest/*",
            vec![
                "imaptest/test",
                "imaptest/test2",
                "imaptest/test3",
                "imaptest/test3/test4",
                "imaptest/test3/test4/test5",
            ],
        ),
        ("imaptest/*test4", vec!["imaptest/test3/test4"]),
        (
            "imaptest/*test*",
            vec![
                "imaptest/test",
                "imaptest/test2",
                "imaptest/test3",
                "imaptest/test3/test4",
                "imaptest/test3/test4/test5",
            ],
        ),
        ("imaptest/%3/%", vec!["imaptest/test3/test4"]),
        ("imaptest/%3/%4", vec!["imaptest/test3/test4"]),
        ("imaptest/%t*4", vec!["imaptest/test3/test4"]),
        ("*st/%3/%4/%5", vec!["imaptest/test3/test4/test5"]),
        (
            "*%*%*%",
            vec![
                "imaptest",
                "imaptest/test",
                "imaptest/test2",
                "imaptest/test3",
                "imaptest/test3/test4",
                "imaptest/test3/test4/test5",
                "foobar/test",
                "foobar/test/test",
                "foobar/test1/test1",
            ],
        ),
        ("foobar*test", vec!["foobar/test", "foobar/test/test"]),
    ] {
        let patterns = vec![pattern.to_string()];
        let mut matched_mailboxes = Vec::new();
        for mailbox in mailboxes {
            if matches_pattern(&patterns, mailbox) {
                matched_mailboxes.push(mailbox);
            }
        }
        assert_eq!(matched_mailboxes, expected_match, "for pattern {}", pattern);
    }
}
