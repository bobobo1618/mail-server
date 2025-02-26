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

use crate::smtp::{inbound::sign::TextConfigContext, session::TestSession, TestConfig};
use directory::config::ConfigDirectory;
use smtp::{
    config::{if_block::ConfigIf, scripts::ConfigSieve, ConfigContext, EnvelopeKey, IfBlock},
    core::{Session, SMTP},
};
use utils::config::{Config, DynValue};

const CONFIG: &str = r#"
[session.mail]
rewrite = [ { all-of = [ { if = "sender-domain", ends-with = ".foobar.net" },
                         { if = "sender", matches = "^([^.]+)@([^.]+)\.(.+)$"}, 
                       ], then = "${1}+${2}@${3}" }, 
            { else = false } ]
script = [ { if = "sender-domain", eq = "foobar.org", then = "mail" }, 
            { else = false } ]

[session.rcpt]
rewrite = [ { all-of = [ { if = "rcpt-domain", eq = "foobar.net" },
                         { if = "rcpt", matches = "^([^.]+)\.([^.]+)@(.+)$"}, 
                       ], then = "${1}+${2}@${3}" }, 
            { else = false } ]
script = [ { if = "rcpt-domain", eq = "foobar.org", then = "rcpt" }, 
            { else = false } ]

[sieve]
from-name = "Sieve Daemon"
from-addr = "sieve@foobar.org"
return-path = ""
hostname = "mx.foobar.org"

[sieve.limits]
redirects = 3
out-messages = 5
received-headers = 50
cpu = 10000
nested-includes = 5
duplicate-expiry = "7d"

[sieve.scripts]
mail = '''
require ["variables", "envelope"];

if allof( envelope :domain :is "from" "foobar.org", 
          envelope :localpart :contains "from" "admin" ) {
     set "envelope.from" "MAILER-DAEMON@foobar.org";
}

'''

rcpt = '''
require ["variables", "envelope", "regex"];

if allof( envelope :localpart :contains "to" ".",
          envelope :regex "to" "(.+)@(.+)$") {
    set :replace "." "" "to" "${1}";
    set "envelope.to" "${to}@${2}";
}

'''

"#;

#[tokio::test]
async fn address_rewrite() {
    /*tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(tracing::Level::TRACE)
            .finish(),
    )
    .unwrap();*/

    // Prepare config
    let available_keys = [
        EnvelopeKey::Sender,
        EnvelopeKey::SenderDomain,
        EnvelopeKey::Recipient,
        EnvelopeKey::RecipientDomain,
    ];
    let mut core = SMTP::test();
    let mut ctx = ConfigContext::new(&[]).parse_signatures();
    let settings = Config::parse(CONFIG).unwrap();
    ctx.directory = settings.parse_directory().unwrap();
    core.sieve = settings.parse_sieve(&mut ctx).unwrap();
    let config = &mut core.session.config;
    config.mail.script = settings
        .parse_if_block::<Option<String>>("session.mail.script", &ctx, &available_keys)
        .unwrap()
        .unwrap_or_default()
        .map_if_block(&ctx.scripts, "session.mail.script", "script")
        .unwrap();
    config.mail.rewrite = settings
        .parse_if_block::<Option<DynValue<EnvelopeKey>>>(
            "session.mail.rewrite",
            &ctx,
            &available_keys,
        )
        .unwrap()
        .unwrap_or_default();
    config.rcpt.script = settings
        .parse_if_block::<Option<String>>("session.rcpt.script", &ctx, &available_keys)
        .unwrap()
        .unwrap_or_default()
        .map_if_block(&ctx.scripts, "session.rcpt.script", "script")
        .unwrap();
    config.rcpt.rewrite = settings
        .parse_if_block::<Option<DynValue<EnvelopeKey>>>(
            "session.rcpt.rewrite",
            &ctx,
            &available_keys,
        )
        .unwrap()
        .unwrap_or_default();
    config.rcpt.relay = IfBlock::new(true);

    // Init session
    let mut session = Session::test(core);
    session.data.remote_ip = "10.0.0.1".parse().unwrap();
    session.eval_session_params().await;
    session.ehlo("mx.doe.org").await;

    // Sender rewrite using regex
    session.mail_from("bill@doe.foobar.net", "250").await;
    assert_eq!(
        session.data.mail_from.as_ref().unwrap().address,
        "bill+doe@foobar.net"
    );
    session.reset();

    // Sender rewrite using sieve
    session.mail_from("this_is_admin@foobar.org", "250").await;
    assert_eq!(
        session.data.mail_from.as_ref().unwrap().address_lcase,
        "mailer-daemon@foobar.org"
    );

    // Recipient rewrite using regex
    session.rcpt_to("mary.smith@foobar.net", "250").await;
    assert_eq!(
        session.data.rcpt_to.last().unwrap().address,
        "mary+smith@foobar.net"
    );

    // Remove duplicates
    session.rcpt_to("mary.smith@foobar.net", "250").await;
    assert_eq!(session.data.rcpt_to.len(), 1);

    // Recipient rewrite using sieve
    session.rcpt_to("m.a.r.y.s.m.i.t.h@foobar.org", "250").await;
    assert_eq!(
        session.data.rcpt_to.last().unwrap().address,
        "marysmith@foobar.org"
    );
}
