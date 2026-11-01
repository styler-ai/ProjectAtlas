//! Retained Windows file policy and protected replacement creation.
//!
//! The safe ACL wrapper lacks control/NULL-SACL helpers; standard file creation
//! cannot supply a security descriptor. This local SDK exception also previews
//! inheritance with the effective token and creates a protected, noninherited handle.
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
use windows_sys::Win32::System::SystemServices::{
    ACCESS_FILTER_SECURITY_INFORMATION, PROCESS_TRUST_LABEL_SECURITY_INFORMATION,
};

/// SACL presence, defaulting, inheritance request/state and protection.
const SACL_CONTROL: u16 = 0x2a30;
/// Only this bit establishes that system policy exists in a filtered query.
const SACL_PRESENT: u16 = 0x0010;
/// A defaulted SACL cannot be reproduced by the selected safe setter.
const SACL_DEFAULTED: u16 = 0x0020;
/// Inheritance protection for the system ACL.
const SACL_PROTECTED: u16 = 0x2000;
/// DACL presence/defaulting and its three native inheritance controls.
const DACL_CONTROL: u16 = 0x150f;
/// Native inheritance controls writable without replacing any ACL entries.
const INHERITANCE_CONTROL: u16 = 0x3f00;

/// Select the system-policy classes supported by native file assignment.
fn assignment_information() -> SecurityInformation {
    SecurityInformation::Sacl
        | SecurityInformation::Label
        | SecurityInformation::Attribute
        | SecurityInformation::Scope
}

/// Query every current SDK class, including classes not supported by assignment.
fn system_information() -> SecurityInformation {
    let known = assignment_information();
    // SAFETY: these are named SECURITY_INFORMATION flags from the current SDK.
    // bitflags 1 permits bits unknown to its older definition, and all three
    // canonical query/format/set wrappers forward these bits unchanged. Never
    // truncate newer selectors: that could silently omit native policy classes.
    unsafe {
        SecurityInformation::from_bits_unchecked(
            known.bits()
                | PROCESS_TRUST_LABEL_SECURITY_INFORMATION as u32
                | ACCESS_FILTER_SECURITY_INFORMATION as u32,
        )
    }
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

/// Capture meaningful DACL controls without conflating other descriptor fields.
pub(super) fn discretionary_control(descriptor: &SecurityDescriptor) -> io::Result<u16> {
    let raw = descriptor_control(descriptor)?;
    if raw & !(DACL_CONTROL | SACL_CONTROL | 0x8000) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported Windows descriptor controls: {raw:04x}"),
        ));
    }
    let control = raw & DACL_CONTROL;
    Ok(if control & 0x0004 == 0 {
        control & !0x0008
    } else {
        control
    })
}

/// SDDL omits defaulted owner/group/DACL metadata; never silently discard it.
fn require_representable_discretionary_control(control: u16) -> io::Result<()> {
    if control & 0x000b != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "defaulted Windows owner/group/DACL metadata cannot be preserved by replacement creation",
        ));
    }
    Ok(())
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
    // The SDK defines DEFAULTED only when PRESENT is set. Separate and combined
    // queries need not preserve this ignored bit for an absent SACL.
    Ok(if control & SACL_PRESENT == 0 {
        control & !SACL_DEFAULTED
    } else {
        control
    })
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

/// Admit assignment only when its native query retains the complete policy.
fn transferable_sacl_image(
    complete: &SecurityDescriptor,
    represented: &SecurityDescriptor,
    complete_image: &OsStr,
) -> io::Result<OsString> {
    let represented_image =
        ConvertSecurityDescriptorToStringSecurityDescriptor(represented, system_information())?;
    require_complete_sacl_image(represented, &represented_image)?;
    if admitted_control(descriptor_control(complete)?, true)?
        != admitted_control(descriptor_control(represented)?, true)?
        || descriptor_sacl(complete)?.is_some() != descriptor_sacl(represented)?.is_some()
        || sacl_entry_count(complete)? != sacl_entry_count(represented)?
        || complete_image != represented_image
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows system policy is not represented by native assignment selectors",
        ));
    }
    Ok(represented_image)
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
            // Select each represented class explicitly. Native retained-handle
            // BACKUP queries fail even with the documented rights on this host.
            system_information()
        } else {
            SecurityInformation::Label
        };
        let mut descriptor = GetSecurityInfo(file, SeObjectType::SE_FILE_OBJECT, query)?;
        // A LABEL query retains the original presence/control bits but filters
        // audit ACEs. Never infer absence from that filtered ACE list or SDDL.
        let control = admitted_control(descriptor_control(&descriptor)?, complete)?;
        let mut sddl = ConvertSecurityDescriptorToStringSecurityDescriptor(
            &descriptor,
            if complete {
                system_information()
            } else {
                query
            },
        )?;
        require_complete_sacl_image(&descriptor, &sddl)?;
        if complete {
            // Query and setter masks have different native contracts. Never
            // silently discard a class omitted by the assignable query, and
            // never request unsupported setter classes for ordinary policy.
            let represented =
                GetSecurityInfo(file, SeObjectType::SE_FILE_OBJECT, assignment_information())?;
            sddl = transferable_sacl_image(&descriptor, &represented, &sddl)?;
            descriptor = represented;
        }
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
        let observed = Self::capture(staged, self.complete)?;
        if self == &observed {
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
        let information = self.assignment_for(&observed)?
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

    /// Metadata-only restoration must not request rights for unchanged classes.
    fn assignment_for(&self, observed: &Self) -> io::Result<SecurityInformation> {
        let same_presence =
            (self.control ^ observed.control) & (SACL_PRESENT | SACL_DEFAULTED) == 0;
        let same_null_state = descriptor_sacl(&self.descriptor)?.is_some()
            == descriptor_sacl(&observed.descriptor)?.is_some();
        if same_presence
            && same_null_state
            && sacl_entries_image(&self.sddl)? == sacl_entries_image(&observed.sddl)?
        {
            // All opaque entries already match. Restore only SACL inheritance;
            // LABEL/ATTRIBUTE/SCOPE would demand unrelated WRITE_OWNER/WRITE_DAC.
            Ok(SecurityInformation::Sacl)
        } else {
            Ok(assignment_information())
        }
    }
}

/// Borrow the exact ordered native ACE image, without interpreting ACE contents.
fn sacl_entries_image(value: &OsStr) -> io::Result<&str> {
    let text = value.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "native SACL image is not text")
    })?;
    Ok(&text[text.find('(').unwrap_or(text.len())..])
}

/// SDK-owned inheritance preview, released by its matching native allocator.
struct InheritedDescriptor(windows_sys::Win32::Security::PSECURITY_DESCRIPTOR);

impl InheritedDescriptor {
    /// Borrow the canonical opaque descriptor while its native allocation lives.
    fn descriptor(&self) -> &SecurityDescriptor {
        // SAFETY: successful CreatePrivateObjectSecurityEx returned this non-null
        // native descriptor; the wrapper is an opaque view of the same SDK type.
        unsafe { &*self.0.cast::<SecurityDescriptor>() }
    }
}

impl Drop for InheritedDescriptor {
    fn drop(&mut self) {
        // SAFETY: this unique allocation came from CreatePrivateObjectSecurityEx;
        // it is never transferred or freed with LocalFree.
        unsafe { windows_sys::Win32::Security::DestroyPrivateObjectSecurity(&raw mut self.0) };
    }
}

/// Open the effective caller token without changing impersonation or privileges.
fn effective_token() -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::Foundation::ERROR_NO_TOKEN;
    use windows_sys::Win32::Security::{TOKEN_DUPLICATE, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
    };
    let mut token = std::ptr::null_mut();
    let rights = TOKEN_QUERY | TOKEN_DUPLICATE;
    // SAFETY: pseudo thread/process handles remain borrowed; the only returned
    // owning handle is written to this live output and wrapped exactly once.
    if unsafe { OpenThreadToken(GetCurrentThread(), rights, 1, &raw mut token) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
            return Err(error);
        }
        // SAFETY: no thread token exists; query the current process token only.
        if unsafe { OpenProcessToken(GetCurrentProcess(), rights, &raw mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: the successful token API transfers one valid owning handle.
    Ok(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(token) })
}

/// Compare native SACL headers while ignoring only preview auto-inheritance.
fn inheritance_system_image(value: &OsStr) -> io::Result<String> {
    let text = value.to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "native SACL image is not text")
    })?;
    let split = text.find('(').unwrap_or(text.len());
    Ok(format!(
        "{}{}",
        text[..split].replace("AI", ""),
        &text[split..]
    ))
}

/// Ignore preview AI and the SDK-undefined default bit only when SACL is absent.
fn inheritance_system_control(control: u16) -> u16 {
    let control = control & SACL_CONTROL & !0x0800;
    if control & SACL_PRESENT == 0 {
        control & !SACL_DEFAULTED
    } else {
        control
    }
}

/// Compare each discretionary component without exposing principals or ACEs.
fn discretionary_components_match(
    descriptor: &SecurityDescriptor,
    expected_image: &OsStr,
) -> io::Result<[bool; 3]> {
    let expected = ConvertStringSecurityDescriptorToSecurityDescriptor(expected_image)?;
    let mut matches = [false; 3];
    for (index, information) in [
        SecurityInformation::Owner,
        SecurityInformation::Group,
        SecurityInformation::Dacl,
    ]
    .into_iter()
    .enumerate()
    {
        matches[index] =
            ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor, information)?
                == ConvertSecurityDescriptorToStringSecurityDescriptor(&expected, information)?;
    }
    Ok(matches)
}

/// Bind a combined descriptor to the already admitted native policy snapshot.
fn snapshot_system_image(
    descriptor: &SecurityDescriptor,
    expected: &super::WindowsAccessSnapshot,
    role: &str,
) -> io::Result<OsString> {
    let complete = expected.system.needs_system_access();
    let discretionary =
        SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl;
    let system = if complete {
        system_information()
    } else {
        SecurityInformation::Label
    };
    let image = ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor, system)?;
    require_complete_sacl_image(descriptor, &image)?;
    let actual_discretionary =
        ConvertSecurityDescriptorToStringSecurityDescriptor(descriptor, discretionary)?;
    let actual_control = admitted_control(descriptor_control(descriptor)?, complete)?;
    let actual_discretionary_control = discretionary_control(descriptor)?;
    if actual_discretionary != expected.discretionary
        || actual_discretionary_control != expected.discretionary_control
        || actual_control != expected.system.control
        || image != expected.system.sddl
    {
        let [owner_matches, group_matches, dacl_matches] =
            discretionary_components_match(descriptor, &expected.discretionary)?;
        return Err(io::Error::other(format!(
            "native policy changed before replacement creation: role={role}, complete={complete}, discretionary_matches={}, owner_matches={owner_matches}, group_matches={group_matches}, dacl_matches={dacl_matches}, expected_dacl_control={:04x}, observed_dacl_control={actual_discretionary_control:04x}, expected_system_control={:04x}, observed_system_control={actual_control:04x}, system_image_matches={}",
            actual_discretionary == expected.discretionary,
            expected.discretionary_control,
            expected.system.control,
            image == expected.system.sddl,
        )));
    }
    Ok(image)
}

/// Assemble the already admitted native components using the SDK codec.
///
/// The source/parent split queries are revalidated by the caller. Exact component
/// and control readback below refuses any loss in this owned representation.
fn snapshot_descriptor(
    expected: &super::WindowsAccessSnapshot,
    role: &str,
) -> io::Result<LocalBox<SecurityDescriptor>> {
    require_representable_discretionary_control(expected.discretionary_control)?;
    let mut encoded = expected.discretionary.clone();
    encoded.push(&expected.system.sddl);
    let mut descriptor = ConvertStringSecurityDescriptorToSecurityDescriptor(&encoded)?;
    let inherited =
        (expected.discretionary_control | expected.system.control) & INHERITANCE_CONTROL;
    // SAFETY: the converter returned this writable SDK-owned descriptor. Only
    // canonical inheritance bits are set; no pointer, ACE or owner is replaced.
    if unsafe {
        windows_sys::Win32::Security::SetSecurityDescriptorControl(
            std::ptr::from_mut::<SecurityDescriptor>(&mut descriptor).cast(),
            INHERITANCE_CONTROL,
            inherited,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    snapshot_system_image(&descriptor, expected, role)?;
    Ok(descriptor)
}

/// Captured source policy and exact parent used to create a replacement safely.
pub(super) struct ReplacementCreation {
    /// Source descriptor with DACL inheritance blocked at initial creation.
    pub(super) descriptor: LocalBox<SecurityDescriptor>,
    /// Original DACL header/order, before adding initial inheritance protection.
    pub(super) dacl_image: OsString,
    /// Retained publication parent; no following security setter is used.
    parent: File,
    /// Parent policy that admitted inheritance restoration.
    parent_policy: super::WindowsAccessSnapshot,
    /// Whether full system-policy rights are already available.
    complete: bool,
    /// Whether restoring the original unprotected DACL needs `WRITE_DAC`.
    restore_inheritance: bool,
}

impl ReplacementCreation {
    /// Preview native inheritance and protect the initial source policy.
    ///
    /// # Errors
    /// Refuses changed, incomplete or differing source/parent policy before staging.
    pub(super) fn capture(
        source: &File,
        directory: &std::path::Path,
        directory_identity: &same_file::Handle,
        expected: &super::WindowsAccessSnapshot,
    ) -> io::Result<Self> {
        use std::os::windows::fs::OpenOptionsExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Security::{
            CreatePrivateObjectSecurityEx, GENERIC_MAPPING, SE_DACL_PROTECTED, SE_SACL_PROTECTED,
            SEF_DACL_AUTO_INHERIT, SEF_SACL_AUTO_INHERIT, SetSecurityDescriptorControl,
        };
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ALL_ACCESS, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        };
        let complete = expected.system.needs_system_access();
        let rights = 0x0002_0080 | if complete { 0x0100_0000 } else { 0 };
        let parent = File::options()
            .access_mode(rights)
            .share_mode(7)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(directory)?;
        if !parent.metadata()?.is_dir()
            || same_file::Handle::from_file(parent.try_clone()?)? != *directory_identity
        {
            return Err(io::Error::other("replacement creation parent changed"));
        }
        if &super::windows_access_snapshot(source, complete)? != expected {
            return Err(io::Error::other(
                "source policy changed before replacement creation",
            ));
        }
        let parent_policy = super::windows_access_snapshot(&parent, complete)?;
        let system = if complete {
            system_information()
        } else {
            SecurityInformation::Label
        };
        let mut descriptor = snapshot_descriptor(expected, "source")?;
        let source_system = expected.system.sddl.clone();
        let parent_descriptor = snapshot_descriptor(&parent_policy, "parent")?;
        if super::windows_access_snapshot(&parent, complete)? != parent_policy {
            return Err(io::Error::other(
                "parent policy changed before replacement creation",
            ));
        }
        let token = effective_token()?;
        let mapping = GENERIC_MAPPING {
            GenericRead: FILE_GENERIC_READ,
            GenericWrite: FILE_GENERIC_WRITE,
            GenericExecute: FILE_GENERIC_EXECUTE,
            GenericAll: FILE_ALL_ACCESS,
        };
        let mut predicted = std::ptr::null_mut();
        // SAFETY: both SDK descriptors and the effective token stay owned and live;
        // this creates a non-container preview with real inheritance/owner checks.
        if unsafe {
            CreatePrivateObjectSecurityEx(
                std::ptr::from_ref::<SecurityDescriptor>(&parent_descriptor)
                    .cast_mut()
                    .cast(),
                std::ptr::from_ref::<SecurityDescriptor>(&descriptor)
                    .cast_mut()
                    .cast(),
                &raw mut predicted,
                std::ptr::null(),
                0,
                SEF_DACL_AUTO_INHERIT | SEF_SACL_AUTO_INHERIT,
                token.as_raw_handle(),
                &raw const mapping,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if predicted.is_null() {
            return Err(io::Error::other(
                "native inheritance preview returned no descriptor",
            ));
        }
        let preview = InheritedDescriptor(predicted);
        let preview = preview.descriptor();
        let dacl_image = ConvertSecurityDescriptorToStringSecurityDescriptor(
            &descriptor,
            SecurityInformation::Dacl,
        )?;
        let preview_dacl = ConvertSecurityDescriptorToStringSecurityDescriptor(
            preview,
            SecurityInformation::Dacl,
        )?;
        let preview_system = ConvertSecurityDescriptorToStringSecurityDescriptor(preview, system)?;
        require_complete_sacl_image(preview, &preview_system)?;
        if descriptor.owner() != preview.owner()
            || descriptor.group() != preview.group()
            || !super::windows_file_dacl_matches(&dacl_image, &preview_dacl)?
            || inheritance_system_control(descriptor_control(&descriptor)?)
                != inheritance_system_control(descriptor_control(preview)?)
            || inheritance_system_image(&source_system)?
                != inheritance_system_image(&preview_system)?
        {
            let difference = super::windows_dacl_difference(&dacl_image, &preview_dacl)?;
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "replacement creation would inherit different native policy: complete={complete}, owner_matches={}, group_matches={}, {difference}, source_control={:04x}, preview_control={:04x}, system_control_matches={}, system_image_matches={}, source_system_entries={}, preview_system_entries={}",
                    descriptor.owner() == preview.owner(),
                    descriptor.group() == preview.group(),
                    descriptor_control(&descriptor)?,
                    descriptor_control(preview)?,
                    inheritance_system_control(descriptor_control(&descriptor)?)
                        == inheritance_system_control(descriptor_control(preview)?),
                    inheritance_system_image(&source_system)?
                        == inheritance_system_image(&preview_system)?,
                    sacl_entry_count(&descriptor)?,
                    sacl_entry_count(preview)?,
                ),
            ));
        }
        let source_control = descriptor_control(&descriptor)?;
        let restore_inheritance = source_control & SE_DACL_PROTECTED == 0;
        let protection = SE_DACL_PROTECTED
            | if source_control & SACL_PRESENT != 0 {
                SE_SACL_PROTECTED
            } else {
                0
            };
        // SAFETY: only inheritance protection on our uniquely owned descriptor
        // changes. Never synthesize a present SACL from absence. Original policy
        // is retained separately for restoration through the original handle.
        if unsafe {
            SetSecurityDescriptorControl(
                std::ptr::from_mut::<SecurityDescriptor>(&mut descriptor).cast(),
                protection,
                protection,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            descriptor,
            dacl_image,
            parent,
            parent_policy,
            complete,
            restore_inheritance,
        })
    }

    /// Revalidate the retained parent's complete admitted policy before restoration.
    ///
    /// # Errors
    /// Refuses policy changes or unavailable native evidence.
    pub(super) fn verify_parent(&self) -> io::Result<()> {
        if super::windows_access_snapshot(&self.parent, self.complete)? != self.parent_policy {
            return Err(io::Error::other(
                "replacement parent policy changed before restoration",
            ));
        }
        Ok(())
    }

    /// Check actual creation policy before callbacks, repair or payload I/O.
    ///
    /// # Errors
    /// Refuses unexpected owner/group, ACL entries, controls or incomplete images.
    pub(super) fn verify_initial(&self, file: &File) -> io::Result<()> {
        let discretionary =
            SecurityInformation::Owner | SecurityInformation::Group | SecurityInformation::Dacl;
        let system = if self.complete {
            system_information()
        } else {
            SecurityInformation::Label
        };
        let observed = GetSecurityInfo(file, SeObjectType::SE_FILE_OBJECT, discretionary)?;
        let observed_system = SystemPolicy::capture(file, self.complete)?;
        let original_dacl = ConvertSecurityDescriptorToStringSecurityDescriptor(
            &self.descriptor,
            SecurityInformation::Dacl,
        )?;
        let observed_dacl = ConvertSecurityDescriptorToStringSecurityDescriptor(
            &observed,
            SecurityInformation::Dacl,
        )?;
        let original_system =
            ConvertSecurityDescriptorToStringSecurityDescriptor(&self.descriptor, system)?;
        require_complete_sacl_image(&self.descriptor, &original_system)?;
        if observed.owner() != self.descriptor.owner()
            || observed.group() != self.descriptor.group()
            || !super::windows_file_dacl_matches(&original_dacl, &observed_dacl)?
            || discretionary_control(&self.descriptor)? & !0x0400
                != discretionary_control(&observed)? & !0x0400
            || inheritance_system_control(descriptor_control(&self.descriptor)?)
                != inheritance_system_control(observed_system.control)
            || inheritance_system_image(&original_system)?
                != inheritance_system_image(&observed_system.sddl)?
        {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "initial replacement stage differs from its protected creation policy",
            ));
        }
        Ok(())
    }

    /// Create one new file with its protected policy and original owning handle.
    ///
    /// # Errors
    /// Returns native creation errors without opening an existing object.
    pub(super) fn create(&self, path: &std::path::Path) -> io::Result<File> {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::io::FromRawHandle;
        use windows_permissions::constants::AccessRights;
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
        use windows_sys::Win32::Storage::FileSystem::{
            CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE,
        };
        self.verify_parent()?;
        let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
        if !path.is_absolute() || name.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "replacement stage requires an absolute NUL-free path",
            ));
        }
        name.push(0);
        let mut rights = (AccessRights::GenericRead
            | AccessRights::GenericWrite
            | AccessRights::Delete
            | AccessRights::ReadControl)
            .bits();
        if self.restore_inheritance {
            rights |= AccessRights::WriteDac.bits();
        }
        if self.restore_inheritance || self.complete {
            rights |= 0x0200_0000; // MAXIMUM_ALLOWED retains optional policy rights.
        }
        if self.complete {
            rights |= AccessRights::AccessSystemSecurity.bits();
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>())
                .map_err(io::Error::other)?,
            lpSecurityDescriptor: std::ptr::from_ref::<SecurityDescriptor>(&self.descriptor)
                .cast_mut()
                .cast(),
            bInheritHandle: 0,
        };
        // SAFETY: all buffers/descriptors stay live for this synchronous call;
        // CREATE_NEW never opens an existing object and the handle is noninherited.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                rights,
                FILE_SHARE_DELETE,
                &raw const attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful create transfers its sole owning handle.
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nonserialized defaulted metadata is refused before native stage creation.
    #[test]
    fn snapshot_assembly_refuses_defaulted_discretionary_metadata() -> io::Result<()> {
        for control in [0x0001, 0x0002, 0x000c, 0x150f] {
            if !require_representable_discretionary_control(control)
                .is_err_and(|error| error.kind() == io::ErrorKind::Unsupported)
            {
                return Err(io::Error::other(
                    "defaulted metadata was silently discarded",
                ));
            }
        }
        for control in [0, 0x0004, 0x0404, 0x1504] {
            require_representable_discretionary_control(control)?;
        }
        Ok(())
    }

    /// Split snapshots retain every native component and control through assembly.
    #[test]
    fn snapshot_descriptor_preserves_native_components_and_query_diagnostics() -> io::Result<()> {
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let output = root.join("component-snapshot");
        std::fs::write(&output, b"component policy")?;
        for path in [&root, &output] {
            let file = File::options()
                .access_mode(0x0002_0080)
                .share_mode(7)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            let snapshot = super::super::windows_access_snapshot(&file, false)?;
            let combined = GetSecurityInfo(
                &file,
                SeObjectType::SE_FILE_OBJECT,
                SecurityInformation::Owner
                    | SecurityInformation::Group
                    | SecurityInformation::Dacl
                    | SecurityInformation::Label,
            )?;
            writeln!(
                io::stderr().lock(),
                "native split/mixed query directory={} owner/group/dacl={:?} split_dacl_control={:04x} mixed_dacl_control={:04x}",
                path == &root,
                discretionary_components_match(&combined, &snapshot.discretionary)?,
                snapshot.discretionary_control,
                discretionary_control(&combined)?
            )?;
            let assembled = snapshot_descriptor(&snapshot, "component-test")?;
            if discretionary_components_match(&assembled, &snapshot.discretionary)? != [true; 3]
                || discretionary_control(&assembled)? != snapshot.discretionary_control
            {
                return Err(io::Error::other(
                    "snapshot assembly changed an admitted native component",
                ));
            }
            snapshot_system_image(&assembled, &snapshot, "component-test")?;
            if super::super::windows_access_snapshot(&file, false)? != snapshot {
                return Err(io::Error::other(
                    "query diagnostics changed the retained native policy",
                ));
            }
        }
        Ok(())
    }

    /// A query subset must not admit assignment after omitting native policy.
    #[test]
    fn assignment_requires_the_same_complete_native_system_policy() -> io::Result<()> {
        let full = ConvertStringSecurityDescriptorToSecurityDescriptor(OsStr::new(
            "S:P(AU;SAFA;FW;;;WD)(ML;;NW;;;HI)",
        ))?;
        let image =
            ConvertSecurityDescriptorToStringSecurityDescriptor(&full, system_information())?;
        require_complete_sacl_image(&full, &image)?;
        let equivalent = ConvertStringSecurityDescriptorToSecurityDescriptor(&image)?;
        if transferable_sacl_image(&full, &equivalent, &image)? != image {
            return Err(io::Error::other(
                "equivalent native policy was not transferable",
            ));
        }
        let filtered = ConvertStringSecurityDescriptorToSecurityDescriptor(OsStr::new(
            "S:P(AU;SAFA;FW;;;WD)",
        ))?;
        if !transferable_sacl_image(&full, &filtered, &image)
            .is_err_and(|source| source.kind() == io::ErrorKind::Unsupported)
        {
            return Err(io::Error::other(
                "a filtered query admitted partial assignment",
            ));
        }
        Ok(())
    }

    /// Unchanged opaque entries need no label, attribute or scope setter rights.
    #[test]
    fn inheritance_only_assignment_does_not_request_unchanged_class_rights() -> io::Result<()> {
        let snapshot = |text: &str| -> io::Result<SystemPolicy> {
            let descriptor = ConvertStringSecurityDescriptorToSecurityDescriptor(text)?;
            let control = admitted_control(descriptor_control(&descriptor)?, true)?;
            let sddl = ConvertSecurityDescriptorToStringSecurityDescriptor(
                &descriptor,
                system_information(),
            )?;
            require_complete_sacl_image(&descriptor, &sddl)?;
            Ok(SystemPolicy {
                descriptor,
                sddl,
                control,
                complete: true,
            })
        };
        for entries in [
            "(AU;SAFA;FW;;;WD)",
            "(ML;;NW;;;HI)",
            r#"(AU;SAFA;FW;;;WD)(RA;;;;;WD;("Claim",TS,0,"alpha"))(SP;;;;;S-1-17-1-2-3-4-5-6-7-8)(ML;;NW;;;HI)"#,
        ] {
            let expected = snapshot(&format!("S:P{entries}"))?;
            let observed = snapshot(&format!("S:AI{entries}"))?;
            if expected.assignment_for(&observed)? != SecurityInformation::Sacl {
                return Err(io::Error::other(
                    "unchanged native policy demanded unrelated class rights",
                ));
            }
        }
        let high = snapshot("S:P(ML;;NW;;;HI)")?;
        let medium = snapshot("S:P(ML;;NW;;;ME)")?;
        if high.assignment_for(&medium)? != assignment_information()
            || snapshot("O:BA")?.assignment_for(&snapshot("S:NO_ACCESS_CONTROL")?)?
                != assignment_information()
        {
            return Err(io::Error::other(
                "changed native policy was reduced to an inheritance-only setter",
            ));
        }
        Ok(())
    }

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
        if inheritance_system_control(SACL_DEFAULTED | 0x0800) != 0
            || inheritance_system_control(SACL_PRESENT | SACL_DEFAULTED)
                != SACL_PRESENT | SACL_DEFAULTED
        {
            return Err(io::Error::other(
                "inheritance preview changed meaningful defaulted SACL state",
            ));
        }
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
            || admitted_control(0x8800 | SACL_DEFAULTED, false)? != 0x0800
            || admitted_control(0x8010 | SACL_PROTECTED, true)? != 0x2010
        {
            return Err(io::Error::other("supported system controls were changed"));
        }
        Ok(())
    }
}
