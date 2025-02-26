/*
 * Copyright (c) 2023 Stalwart Labs Ltd.
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

use std::fmt::Debug;

use directory::{Principal, Type};
use mail_send::Credentials;

use crate::directory::parse_config;

#[tokio::test]
async fn ldap_directory() {
    // Enable logging
    /*tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(tracing::Level::DEBUG)
            .finish(),
    )
    .unwrap();*/

    // Obtain directory handle
    let mut config = parse_config();
    let lookups = config.lookups;
    let handle = config.directories.remove("ldap").unwrap();

    // Text lookup
    assert!(lookups
        .get("ldap/domains")
        .unwrap()
        .contains("example.org")
        .await
        .unwrap());

    // Test authentication
    assert_eq!(
        handle
            .authenticate(&Credentials::Plain {
                username: "john".to_string(),
                secret: "12345".to_string()
            })
            .await
            .unwrap()
            .unwrap(),
        Principal {
            name: "john".to_string(),
            description: "John Doe".to_string().into(),
            secrets: vec!["12345".to_string()],
            typ: Type::Individual,
            member_of: vec!["sales".to_string()],
            ..Default::default()
        }
    );
    assert_eq!(
        handle
            .authenticate(&Credentials::Plain {
                username: "bill".to_string(),
                secret: "password".to_string()
            })
            .await
            .unwrap()
            .unwrap(),
        Principal {
            name: "bill".to_string(),
            description: "Bill Foobar".to_string().into(),
            secrets: vec![
                "$2y$05$bvIG6Nmid91Mu9RcmmWZfO5HJIMCT8riNW0hEp8f6/FuA2/mHZFpe".to_string()
            ],
            typ: Type::Individual,
            quota: 500000,
            ..Default::default()
        }
    );
    assert!(handle
        .authenticate(&Credentials::Plain {
            username: "bill".to_string(),
            secret: "invalid".to_string()
        })
        .await
        .unwrap()
        .is_none());

    // Get user by name
    let mut principal = handle.principal("jane").await.unwrap().unwrap();
    principal.member_of.sort_unstable();
    assert_eq!(
        principal,
        Principal {
            name: "jane".to_string(),
            description: "Jane Doe".to_string().into(),
            typ: Type::Individual,
            secrets: vec!["abcde".to_string()],
            member_of: vec!["sales".to_string(), "support".to_string()],
            ..Default::default()
        }
    );

    // Get group by name
    assert_eq!(
        handle.principal("sales").await.unwrap().unwrap(),
        Principal {
            name: "sales".to_string(),
            description: "sales".to_string().into(),
            typ: Type::Group,
            ..Default::default()
        }
    );

    // Emails by id
    compare_sorted(
        handle.emails_by_name("john").await.unwrap(),
        vec![
            "john@example.org".to_string(),
            "john.doe@example.org".to_string(),
        ],
    );
    compare_sorted(
        handle.emails_by_name("bill").await.unwrap(),
        vec!["bill@example.org".to_string()],
    );

    // Ids by email
    compare_sorted(
        handle.names_by_email("jane@example.org").await.unwrap(),
        vec!["jane".to_string()],
    );
    compare_sorted(
        handle
            .names_by_email("jane+alias@example.org")
            .await
            .unwrap(),
        vec!["jane".to_string()],
    );
    compare_sorted(
        handle.names_by_email("info@example.org").await.unwrap(),
        vec!["john".to_string(), "jane".to_string(), "bill".to_string()],
    );
    compare_sorted(
        handle
            .names_by_email("info+alias@example.org")
            .await
            .unwrap(),
        vec!["john".to_string(), "jane".to_string(), "bill".to_string()],
    );
    compare_sorted(
        handle.names_by_email("unknown@example.org").await.unwrap(),
        Vec::<String>::new(),
    );
    assert_eq!(
        handle
            .names_by_email("anything@catchall.org")
            .await
            .unwrap(),
        vec!["robert".to_string()]
    );

    // Domain validation
    assert!(handle.is_local_domain("example.org").await.unwrap());
    assert!(!handle.is_local_domain("other.org").await.unwrap());

    // RCPT TO
    assert!(handle.rcpt("jane@example.org").await.unwrap());
    assert!(handle.rcpt("info@example.org").await.unwrap());
    assert!(handle.rcpt("jane+alias@example.org").await.unwrap());
    assert!(handle.rcpt("info+alias@example.org").await.unwrap());
    assert!(handle.rcpt("random_user@catchall.org").await.unwrap());
    assert!(!handle.rcpt("invalid@example.org").await.unwrap());

    // VRFY
    compare_sorted(
        handle.vrfy("jane").await.unwrap(),
        vec!["jane@example.org".to_string()],
    );
    compare_sorted(
        handle.vrfy("john").await.unwrap(),
        vec!["john@example.org".to_string()],
    );
    compare_sorted(
        handle.vrfy("jane+alias@example").await.unwrap(),
        vec!["jane@example.org".to_string()],
    );
    compare_sorted(handle.vrfy("info").await.unwrap(), Vec::<String>::new());
    compare_sorted(handle.vrfy("invalid").await.unwrap(), Vec::<String>::new());

    // EXPN
    compare_sorted(
        handle.expn("info@example.org").await.unwrap(),
        vec![
            "bill@example.org".to_string(),
            "jane@example.org".to_string(),
            "john@example.org".to_string(),
        ],
    );
    compare_sorted(
        handle.expn("john@example.org").await.unwrap(),
        Vec::<String>::new(),
    );
}

fn compare_sorted<T: Eq + Debug>(v1: Vec<T>, v2: Vec<T>) {
    for val in v1.iter() {
        assert!(v2.contains(val), "{v1:?} != {v2:?}");
    }

    for val in v2.iter() {
        assert!(v1.contains(val), "{v1:?} != {v2:?}");
    }
}
