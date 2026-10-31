//! Retained-handle Windows system-policy snapshots without privilege changes.
//!
//! The canonical safe ACL wrapper omits the descriptor-control getter. Only that
//! read-only SDK call needs this reviewed platform-local unsafe exception.
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::fs::File;
use std::io;
use windows_permissions::constants::{SeObjectType, SecurityInformation};
use windows_permissions::wrappers::{
    ConvertSecurityDescriptorToStringSecurityDescriptor, GetSecurityInfo, SetSecurityInfo,
};
use windows_permissions::{LocalBox, SecurityDescriptor};

/// SACL presence, defaulting, inheritance request/state and protection.
const SACL_CONTROL: u16 = 0x2a30;
/// A defaulted SACL cannot be reproduced by the selected safe setter.
const SACL_DEFAULTED: u16 = 0x0020;
/// Inheritance protection for the system ACL.
const SACL_PROTECTED: u16 = 0x2000;

#[link(name = "advapi32")]
unsafe extern "system" {
    /// Read control flags from a valid borrowed native security descriptor.
    #[link_name = "GetSecurityDescriptorControl"]
    fn get_security_descriptor_control(
        descriptor: *const SecurityDescriptor,
        control: *mut u16,
        revision: *mut u32,
    ) -> i32;
}

/// Read the SDK control word without inspecting a descriptor's memory layout.
fn descriptor_control(descriptor: &SecurityDescriptor) -> io::Result<u16> {
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: GetSecurityInfo owns a valid descriptor for the borrowed lifetime;
    // the API reads it and writes only these correctly sized live stack values.
    if unsafe {
        get_security_descriptor_control(
            std::ptr::from_ref(descriptor),
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
    if control & SACL_DEFAULTED != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "defaulted Windows system ACL cannot be preserved",
        ));
    }
    if !complete && control != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Windows system ACL preservation requires an already available full-policy handle: native_control={control:04x}"
            ),
        ));
    }
    Ok(control)
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
        let information = if complete {
            SecurityInformation::Sacl | SecurityInformation::Label
        } else {
            SecurityInformation::Label
        };
        let descriptor = GetSecurityInfo(file, SeObjectType::SE_FILE_OBJECT, information)?;
        // A LABEL query retains the original presence/control bits but filters
        // audit ACEs. Never infer absence from that filtered ACE list or SDDL.
        let control = admitted_control(descriptor_control(&descriptor)?, complete)?;
        let sddl = ConvertSecurityDescriptorToStringSecurityDescriptor(
            &descriptor,
            SecurityInformation::Sacl,
        )?;
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
        let text = self.sddl.to_str().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "native SACL header is not text")
        })?;
        let flags = if text.is_empty() {
            None
        } else {
            Some(
                text.strip_prefix("S:")
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "native SACL header is invalid")
                    })?
                    .split('(')
                    .next()
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "native SACL flags are missing")
                    })?,
            )
        };
        // The dependency's accessor panics on present+NULL. Native absence/NULL
        // has no ACL pointer; do not send either through that accessor.
        let sacl = if flags.is_none_or(|flags| flags.contains("NO_ACCESS_CONTROL")) {
            None
        } else {
            self.descriptor.sacl()
        };
        let information = SecurityInformation::Sacl
            | SecurityInformation::Label
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
            || admitted_control(0x8010 | SACL_PROTECTED, true)? != 0x2010
        {
            return Err(io::Error::other("supported system controls were changed"));
        }
        Ok(())
    }
}
