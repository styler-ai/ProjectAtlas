//! Retained-handle Windows system-policy snapshots without privilege changes.
//!
//! The canonical safe ACL wrapper omits the control getter and panics on NULL
//! SACLs. Canonical SDK getters and empty-ACL initialization use this local exception.
#![allow(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use windows_permissions::constants::{SeObjectType, SecurityInformation};
use windows_permissions::wrappers::{
    ConvertSecurityDescriptorToStringSecurityDescriptor,
    ConvertStringSecurityDescriptorToSecurityDescriptor, GetAclInformationSize, GetSecurityInfo,
    SetSecurityInfo,
};
use windows_permissions::{Acl, LocalBox, SecurityDescriptor};
use windows_sys::Win32::Security::{
    ACL, ACL_REVISION, GetSecurityDescriptorControl, GetSecurityDescriptorSacl, InitializeAcl,
};

/// SACL presence, defaulting, inheritance request/state and protection.
const SACL_CONTROL: u16 = 0x2a30;
/// Only this bit establishes that system policy exists in a filtered query.
const SACL_PRESENT: u16 = 0x0010;
/// A defaulted SACL cannot be reproduced by the selected safe setter.
const SACL_DEFAULTED: u16 = 0x0020;
/// Inheritance protection for the system ACL.
const SACL_PROTECTED: u16 = 0x2000;

/// Select every supported system-policy class for serialization and assignment.
fn system_information() -> SecurityInformation {
    SecurityInformation::Sacl
        | SecurityInformation::Label
        | SecurityInformation::Attribute
        | SecurityInformation::Scope
}

/// SDK-initialized zero-entry ACL, keeping its opaque borrow valid during a setter.
#[repr(C, align(4))]
struct EmptySacl(ACL);

impl EmptySacl {
    /// Initialize canonical aligned storage, propagating a native refusal.
    fn new() -> io::Result<Self> {
        let mut empty = Self(ACL::default());
        let size = u32::try_from(std::mem::size_of::<ACL>()).map_err(io::Error::other)?;
        // SAFETY: SDK ACL storage is aligned, writable and exactly `size` bytes.
        // InitializeAcl creates a valid empty ACL without storing external pointers.
        if unsafe { InitializeAcl(&raw mut empty.0, size, ACL_REVISION) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(empty)
    }

    /// Borrow the initialized SDK ACL through the safe wrapper's opaque ABI type.
    fn as_acl(&self) -> &Acl {
        // SAFETY: successful SDK initialization is the only constructor. Acl is
        // the wrapper's opaque repr(C) view of this same native ACL; its borrow
        // cannot outlive this owner and no Rust code interprets the layout.
        unsafe { &*std::ptr::from_ref(&self.0).cast::<Acl>() }
    }
}

/// Read the SDK control word without inspecting a descriptor's memory layout.
fn descriptor_control(descriptor: &SecurityDescriptor) -> io::Result<u16> {
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: GetSecurityInfo owns a valid descriptor for the borrowed lifetime;
    // the API reads it and writes only these correctly sized live stack values.
    if unsafe {
        GetSecurityDescriptorControl(
            std::ptr::from_ref(descriptor).cast_mut().cast(),
            &raw mut control,
            &raw mut revision,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if revision != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native security descriptor revision is unsupported",
        ));
    }
    Ok(control)
}

/// Admit only complete supported policy or a proven absent partial snapshot.
fn admitted_control(control: u16, complete: bool) -> io::Result<u16> {
    let control = control & SACL_CONTROL;
    if control & SACL_PRESENT != 0 && control & SACL_DEFAULTED != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "defaulted Windows system ACL cannot be preserved",
        ));
    }
    if !complete && control & SACL_PRESENT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Windows system ACL preservation requires an already available full-policy handle: native_control={control:04x}"
            ),
        ));
    }
    Ok(control)
}

/// Borrow only a present non-NULL native ACL from its owning descriptor.
fn descriptor_sacl(descriptor: &SecurityDescriptor) -> io::Result<Option<&Acl>> {
    let mut present = 0;
    let mut defaulted = 0;
    let mut sacl = std::ptr::null_mut();
    // SAFETY: the borrowed SDK-owned descriptor is valid. The API only reads it
    // and writes these live BOOL/pointer outputs. A returned ACL remains owned
    // by that descriptor; do not inspect the pointer when presence is false.
    if unsafe {
        GetSecurityDescriptorSacl(
            std::ptr::from_ref(descriptor).cast_mut().cast(),
            &raw mut present,
            &raw mut sacl,
            &raw mut defaulted,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if present == 0 || sacl.is_null() {
        return Ok(None);
    }
    // SAFETY: successful native query returned a non-NULL ACL in a valid
    // descriptor. Borrow it only for that descriptor's lifetime, never mutate it.
    Ok(Some(unsafe { &*sacl.cast::<Acl>() }))
}

/// Count native entries without trusting an image to classify NULL policy.
fn sacl_entry_count(descriptor: &SecurityDescriptor) -> io::Result<u32> {
    Ok(descriptor_sacl(descriptor)?
        .map(GetAclInformationSize)
        .transpose()?
        .map_or(0, |size| size.AceCount))
}

/// Refuse a comparison image whose selected conversion discarded native entries.
///
/// Equal counts do not establish semantic fidelity; content/order still use the
/// native SDDL comparison and source/stage verification at their existing boundary.
fn require_complete_sacl_image(descriptor: &SecurityDescriptor, image: &OsStr) -> io::Result<()> {
    let entries = sacl_entry_count(descriptor)?;
    if entries == 0 {
        return Ok(());
    }
    let decoded = ConvertStringSecurityDescriptorToSecurityDescriptor(image)?;
    if sacl_entry_count(&decoded)? != entries {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native SACL serialization omitted policy entries",
        ));
    }
    Ok(())
}

/// Native system ACL and its control state, retained independently of its name.
pub(super) struct SystemPolicy {
    /// SDK-owned opaque descriptor; ordered ACEs are never decoded by `ProjectAtlas`.
    descriptor: LocalBox<SecurityDescriptor>,
    /// Native SDDL is a comparison image, including NULL versus empty ACL state.
    sddl: OsString,
    /// Relevant native controls, including bits omitted by SDDL.
    control: u16,
    /// Whether the retained file handle can query complete system policy.
    complete: bool,
}

impl PartialEq for SystemPolicy {
    fn eq(&self, other: &Self) -> bool {
        self.control == other.control && self.sddl == other.sddl
    }
}

impl Eq for SystemPolicy {}

impl SystemPolicy {
    /// Capture full policy, or prove absence using the label-query control word.
    ///
    /// # Errors
    /// Returns native query failures or refuses policy that cannot be preserved.
    pub(super) fn capture(file: &File, complete: bool) -> io::Result<Self> {
        let query = if complete {
            // The SDK's individual selectors filter ACE classes. BACKUP is a
            // complete descriptor query, requiring only the RC + system rights
            // already held here; never use it for serialization or assignment.
            SecurityInformation::Backup
        } else {
            SecurityInformation::Label
        };
        let descriptor = GetSecurityInfo(file, SeObjectType::SE_FILE_OBJECT, query)?;
        // A LABEL query retains the original presence/control bits but filters
        // audit ACEs. Never infer absence from that filtered ACE list or SDDL.
        let control = admitted_control(descriptor_control(&descriptor)?, complete)?;
        let sddl = ConvertSecurityDescriptorToStringSecurityDescriptor(
            &descriptor,
            if complete {
                system_information()
            } else {
                query
            },
        )?;
        require_complete_sacl_image(&descriptor, &sddl)?;
        Ok(Self {
            descriptor,
            sddl,
            control,
            complete,
        })
    }

    /// Acquire matching system-policy rights when creating the original stage.
    pub(super) fn needs_system_access(&self) -> bool {
        self.complete
    }

    /// Recheck policy through the same query class and retained object handle.
    ///
    /// # Errors
    /// Returns native errors or refuses changed system policy.
    pub(super) fn verify(&self, file: &File) -> io::Result<()> {
        let observed = Self::capture(file, self.complete)?;
        if self != &observed {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "Windows system access policy changed: expected_control={:04x}, observed_control={:04x}, native_sddl_matches={}",
                    self.control,
                    observed.control,
                    self.sddl == observed.sddl,
                ),
            ));
        }
        Ok(())
    }

    /// Preserve opaque system policy before payload I/O using the held stage.
    ///
    /// # Errors
    /// Returns native failures or refuses an unverifiable/mismatched stage.
    pub(super) fn apply(&self, staged: &File) -> io::Result<()> {
        if self == &Self::capture(staged, self.complete)? {
            return Ok(());
        }
        if !self.complete {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "staged Windows system policy cannot be preserved without full-policy access",
            ));
        }
        let sacl = descriptor_sacl(&self.descriptor)?;
        let empty =
            if sacl.is_none() && self.control & (SACL_PRESENT | SACL_PROTECTED) == SACL_PRESENT {
                Some(EmptySacl::new()?)
            } else {
                None
            };
        let sacl = sacl.or_else(|| empty.as_ref().map(EmptySacl::as_acl));
        let information = system_information()
            | if self.control & SACL_PROTECTED != 0 {
                SecurityInformation::ProtectedSacl
            } else {
                SecurityInformation::UnprotectedSacl
            };
        let mut retained = staged.try_clone()?;
        SetSecurityInfo(
            &mut retained,
            SeObjectType::SE_FILE_OBJECT,
            information,
            None,
            None,
            None,
            sacl,
        )?;
        self.verify(&retained)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The safe wrapper recognizes the SDK-owned zero-entry ACL representation.
    #[test]
    fn sdk_initialized_empty_sacl_has_no_native_entries() -> io::Result<()> {
        let empty = EmptySacl::new()?;
        if GetAclInformationSize(empty.as_acl())?.AceCount != 0 {
            return Err(io::Error::other("SDK empty ACL contains native entries"));
        }
        Ok(())
    }

    /// A native filtered image must never be accepted as complete system policy.
    #[test]
    fn sacl_comparison_rejects_serialization_that_omits_mandatory_labels() -> io::Result<()> {
        for policy in ["S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)", "S:P(ML;;NW;;;HI)"] {
            let descriptor = ConvertStringSecurityDescriptorToSecurityDescriptor(policy)?;
            let filtered = ConvertSecurityDescriptorToStringSecurityDescriptor(
                &descriptor,
                SecurityInformation::Sacl,
            )?;
            for omitted in [&*filtered, OsStr::new("S:NO_ACCESS_CONTROL")] {
                if !require_complete_sacl_image(&descriptor, omitted)
                    .is_err_and(|source| source.kind() == io::ErrorKind::Unsupported)
                {
                    return Err(io::Error::other("filtered native SACL image was admitted"));
                }
            }
            let complete = ConvertSecurityDescriptorToStringSecurityDescriptor(
                &descriptor,
                SecurityInformation::Sacl | SecurityInformation::Label,
            )?;
            require_complete_sacl_image(&descriptor, &complete)?;
        }
        Ok(())
    }

    /// Defaulted controls are refused even when the caller has full SACL rights.
    #[test]
    fn defaulted_or_hidden_system_policy_is_never_admitted_as_absent() -> io::Result<()> {
        let defaulted = admitted_control(0x8010 | SACL_DEFAULTED, true);
        if !defaulted.is_err_and(|source| source.kind() == io::ErrorKind::Unsupported) {
            return Err(io::Error::other("defaulted SACL was admitted"));
        }
        let hidden = admitted_control(0x8010, false);
        if !hidden.is_err_and(|source| source.kind() == io::ErrorKind::PermissionDenied) {
            return Err(io::Error::other("hidden SACL was admitted as absent"));
        }
        if admitted_control(0x8000, false)? != 0
            || admitted_control(0x8800, false)? != 0x0800
            || admitted_control(0x8800 | SACL_DEFAULTED, false)? != 0x0800 | SACL_DEFAULTED
            || admitted_control(0x8010 | SACL_PROTECTED, true)? != 0x2010
        {
            return Err(io::Error::other("supported system controls were changed"));
        }
        Ok(())
    }
}
