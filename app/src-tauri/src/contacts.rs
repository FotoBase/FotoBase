//! Local contacts link (TASK-05-011, PLAN §2G "Contacts link — local
//! contacts only", §6 P3 People). OS contacts access behind a small trait:
//! the catalog stores only the contact's *reference* (crates/catalog
//! `link_contact`), this module resolves references to name/email from the
//! OS contacts store on display. Strictly local — no network client exists
//! anywhere in the contacts path and none may be added (test-enforced by
//! `tests/no_network_clients.rs`).
//!
//! The macOS backend drives the Contacts framework FFI. The workspace
//! denies `unsafe_code`; the escape is scoped to this module alone
//! (the `display_profile` precedent).
#![allow(unsafe_code)]

/// One local OS contact resolved from the contacts store. `id` is the
/// provider identifier Fotobase stores in the catalog — never the data
/// itself, which stays in the OS store.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Contact {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

/// The local contacts store, abstracted so tests inject a mock and each
/// platform backs it with its own API (macOS Contacts framework now;
/// Windows People / Linux desktop portal behind the same trait).
pub trait ContactsProvider: Send + Sync {
    /// Contacts whose name matches `query` (prefix or substring, store's
    /// choice). Empty on a denied/undetermined permission — never an error
    /// for a permission state.
    fn search(&self, query: &str) -> Result<Vec<Contact>, String>;
    /// Resolve one reference. `Ok(None)` when the contact no longer exists
    /// (deleted on the OS side): the mapping survives, the display shows
    /// nothing.
    fn get(&self, id: &str) -> Result<Option<Contact>, String>;
}

/// The platform's provider. `None` where the platform backend is not
/// implemented yet — the People Manager hides the link UI then.
pub fn system_provider() -> Option<std::sync::Arc<dyn ContactsProvider>> {
    #[cfg(target_os = "macos")]
    {
        Some(std::sync::Arc::new(macos::MacContacts))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{Contact, ContactsProvider};
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2_contacts::{
        CNContact, CNContactEmailAddressesKey, CNContactFamilyNameKey, CNContactGivenNameKey,
        CNContactIdentifierKey, CNContactOrganizationNameKey, CNContactStore, CNEntityType,
        CNKeyDescriptor,
    };
    use objc2_foundation::{NSArray, NSError, NSString};

    /// macOS Contacts framework via `CNContactStore`. Access is requested
    /// lazily on the first call (the TCC prompt); a denied or undetermined
    /// permission yields an empty result rather than an error.
    pub struct MacContacts;

    /// The keys we display: identifier (the stored reference), name parts
    /// and the email addresses.
    fn keys_to_fetch() -> Retained<NSArray<ProtocolObject<dyn CNKeyDescriptor>>> {
        unsafe {
            NSArray::from_slice(&[
                ProtocolObject::from_ref(CNContactIdentifierKey),
                ProtocolObject::from_ref(CNContactGivenNameKey),
                ProtocolObject::from_ref(CNContactFamilyNameKey),
                ProtocolObject::from_ref(CNContactOrganizationNameKey),
                ProtocolObject::from_ref(CNContactEmailAddressesKey),
            ])
        }
    }

    /// Blocking access request: the completion handler fires on an arbitrary
    /// queue, we wait on a channel. `Ok(false)` = denied.
    fn request_access(store: &CNContactStore) -> Result<bool, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let block = block2::RcBlock::new(move |granted: Bool, _err: *mut NSError| {
            let _ = tx.send(granted.is_true());
        });
        unsafe {
            store.requestAccessForEntityType_completionHandler(CNEntityType::Contacts, &block);
        }
        rx.recv().map_err(|_| "contacts access request dropped".to_string())
    }

    fn display_name(c: &CNContact) -> String {
        let (given, family, org) = unsafe {
            (
                c.givenName().to_string(),
                c.familyName().to_string(),
                c.organizationName().to_string(),
            )
        };
        let full = format!("{given} {family}").trim().to_string();
        if full.is_empty() {
            org
        } else {
            full
        }
    }

    fn primary_email(c: &CNContact) -> Option<String> {
        let emails = unsafe { c.emailAddresses() };
        emails.iter().next().map(|e| unsafe { e.value().to_string() })
    }

    fn contact(c: &CNContact) -> Contact {
        Contact {
            id: unsafe { c.identifier().to_string() },
            name: display_name(c),
            email: primary_email(c),
        }
    }

    impl ContactsProvider for MacContacts {
        fn search(&self, query: &str) -> Result<Vec<Contact>, String> {
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }
            let store = unsafe { CNContactStore::new() };
            if !request_access(&store)? {
                return Ok(Vec::new());
            }
            let predicate = unsafe {
                CNContact::predicateForContactsMatchingName(&NSString::from_str(query.trim()))
            };
            let found = unsafe {
                store.unifiedContactsMatchingPredicate_keysToFetch_error(
                    &predicate,
                    &keys_to_fetch(),
                )
            }
            .map_err(|e| e.localizedDescription().to_string())?;
            Ok(found.iter().map(|c| contact(&c)).collect())
        }

        fn get(&self, id: &str) -> Result<Option<Contact>, String> {
            let store = unsafe { CNContactStore::new() };
            if !request_access(&store)? {
                return Ok(None);
            }
            let id_ns = NSString::from_str(id);
            let predicate = unsafe {
                CNContact::predicateForContactsWithIdentifiers(&NSArray::from_slice(&[&*id_ns]))
            };
            let found = unsafe {
                store.unifiedContactsMatchingPredicate_keysToFetch_error(
                    &predicate,
                    &keys_to_fetch(),
                )
            }
            .map_err(|e| e.localizedDescription().to_string())?;
            Ok(found.iter().next().map(|c| contact(&c)))
        }
    }
}
