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

use jmap_proto::{
    error::{method::MethodError, set::SetError},
    method::set::{RequestArguments, SetRequest, SetResponse},
    object::Object,
    response::references::EvalObjectReferences,
    types::{
        collection::Collection,
        property::Property,
        value::{MaybePatchValue, Value},
    },
};
use store::write::{log::ChangeLogBuilder, BatchBuilder, F_CLEAR, F_VALUE};

use crate::{auth::AccessToken, JMAP};

impl JMAP {
    pub async fn identity_set(
        &self,
        mut request: SetRequest<RequestArguments>,
        access_token: &AccessToken,
    ) -> Result<SetResponse, MethodError> {
        let account_id = request.account_id.document_id();
        let mut identity_ids = self
            .get_document_ids(account_id, Collection::Identity)
            .await?
            .unwrap_or_default();
        let mut response = SetResponse::from_request(&request, self.config.set_max_objects)?;
        let will_destroy = request.unwrap_destroy();

        // Process creates
        let mut changes = ChangeLogBuilder::new();
        'create: for (id, object) in request.unwrap_create() {
            let mut identity = Object::with_capacity(object.properties.len());

            for (property, value) in object.properties {
                match response
                    .eval_object_references(value)
                    .and_then(|value| validate_identity_value(&property, value, None))
                {
                    Ok(Value::Null) => (),
                    Ok(value) => {
                        identity.set(property, value);
                    }
                    Err(err) => {
                        response.not_created.append(id, err);
                        continue 'create;
                    }
                }
            }

            // Validate email address
            if let Value::Text(email) = identity.get(&Property::Email) {
                let account_name = if access_token.primary_id == account_id {
                    access_token.name.clone()
                } else {
                    self.get_account_name(account_id).await?.unwrap_or_default()
                };

                if !self
                    .directory
                    .emails_by_name(&account_name)
                    .await
                    .unwrap_or_default()
                    .contains(email)
                {
                    response.not_created.append(
                        id,
                        SetError::invalid_properties()
                            .with_property(Property::Email)
                            .with_description(
                                "E-mail address not configured for this account.".to_string(),
                            ),
                    );
                    continue 'create;
                }
            } else {
                response.not_created.append(
                    id,
                    SetError::invalid_properties()
                        .with_property(Property::Email)
                        .with_description("Missing e-mail address."),
                );
                continue 'create;
            }

            // Insert record
            let mut batch = BatchBuilder::new();
            let document_id = self
                .assign_document_id(account_id, Collection::Identity)
                .await?;
            batch
                .with_account_id(account_id)
                .with_collection(Collection::Identity)
                .create_document(document_id)
                .value(Property::Value, identity, F_VALUE);
            identity_ids.insert(document_id);
            self.write_batch(batch).await?;
            changes.log_insert(Collection::Identity, document_id);
            response.created(id, document_id);
        }

        // Process updates
        'update: for (id, object) in request.unwrap_update() {
            // Make sure id won't be destroyed
            if will_destroy.contains(&id) {
                response.not_updated.append(id, SetError::will_destroy());
                continue 'update;
            }

            // Obtain identity
            let document_id = id.document_id();
            let mut identity = if let Some(identity) = self
                .get_property::<Object<Value>>(
                    account_id,
                    Collection::Identity,
                    document_id,
                    Property::Value,
                )
                .await?
            {
                identity
            } else {
                response.not_updated.append(id, SetError::not_found());
                continue 'update;
            };

            for (property, value) in object.properties {
                match response
                    .eval_object_references(value)
                    .and_then(|value| validate_identity_value(&property, value, Some(&identity)))
                {
                    Ok(Value::Null) => {
                        identity.remove(&property);
                    }
                    Ok(value) => {
                        identity.set(property, value);
                    }
                    Err(err) => {
                        response.not_updated.append(id, err);
                        continue 'update;
                    }
                };
            }

            // Update record
            let mut batch = BatchBuilder::new();
            batch
                .with_account_id(account_id)
                .with_collection(Collection::Identity)
                .update_document(document_id)
                .value(Property::Value, identity, F_VALUE);
            self.write_batch(batch).await?;
            changes.log_update(Collection::Identity, document_id);
            response.updated.append(id, None);
        }

        // Process deletions
        for id in will_destroy {
            let document_id = id.document_id();
            if identity_ids.contains(document_id) {
                // Update record
                let mut batch = BatchBuilder::new();
                batch
                    .with_account_id(account_id)
                    .with_collection(Collection::Identity)
                    .delete_document(document_id)
                    .value(Property::Value, (), F_VALUE | F_CLEAR);
                self.write_batch(batch).await?;
                changes.log_delete(Collection::Identity, document_id);
                response.destroyed.push(id);
            } else {
                response.not_destroyed.append(id, SetError::not_found());
            }
        }

        // Write changes
        if !changes.is_empty() {
            response.new_state = Some(self.commit_changes(account_id, changes).await?.into());
        }

        Ok(response)
    }
}

fn validate_identity_value(
    property: &Property,
    value: MaybePatchValue,
    current: Option<&Object<Value>>,
) -> Result<Value, SetError> {
    Ok(match (property, value) {
        (Property::Name, MaybePatchValue::Value(Value::Text(value))) if value.len() < 255 => {
            Value::Text(value)
        }
        (Property::Email, MaybePatchValue::Value(Value::Text(value)))
            if current.is_none() && value.len() < 255 =>
        {
            Value::Text(sanitize_email(&value).ok_or_else(|| {
                SetError::invalid_properties()
                    .with_property(Property::Email)
                    .with_description("Invalid e-mail address.")
            })?)
        }
        (
            Property::TextSignature | Property::HtmlSignature,
            MaybePatchValue::Value(Value::Text(value)),
        ) if value.len() < 2048 => Value::Text(value),
        (Property::ReplyTo | Property::Bcc, MaybePatchValue::Value(Value::List(value))) => {
            for addr in &value {
                let mut is_valid = false;
                if let Value::Object(obj) = addr {
                    for (key, value) in &obj.properties {
                        match (key, value) {
                            (Property::Email, Value::Text(value)) if value.len() < 255 => {
                                is_valid = true
                            }
                            (Property::Name, Value::Text(value)) if value.len() < 255 => (),
                            (Property::Name, Value::Null) => (),
                            _ => {
                                is_valid = false;
                                break;
                            }
                        }
                    }
                }

                if !is_valid {
                    return Err(SetError::invalid_properties()
                        .with_property(property.clone())
                        .with_description("Invalid e-mail address object."));
                }
            }

            Value::List(value)
        }
        (
            Property::Name
            | Property::TextSignature
            | Property::HtmlSignature
            | Property::ReplyTo
            | Property::Bcc,
            MaybePatchValue::Value(Value::Null),
        ) => Value::Null,

        (property, _) => {
            return Err(SetError::invalid_properties()
                .with_property(property.clone())
                .with_description("Field could not be set."));
        }
    })
}

// Basic email sanitizer
pub fn sanitize_email(email: &str) -> Option<String> {
    let mut result = String::with_capacity(email.len());
    let mut found_local = false;
    let mut found_domain = false;
    let mut last_ch = char::from(0);

    for ch in email.chars() {
        if !ch.is_whitespace() {
            if ch == '@' {
                if !result.is_empty() && !found_local {
                    found_local = true;
                } else {
                    return None;
                }
            } else if ch == '.' {
                if !(last_ch.is_alphanumeric() || last_ch == '-' || last_ch == '_') {
                    return None;
                } else if found_local {
                    found_domain = true;
                }
            }
            last_ch = ch;
            for ch in ch.to_lowercase() {
                result.push(ch);
            }
        }
    }

    if found_domain && last_ch != '.' {
        Some(result)
    } else {
        None
    }
}
