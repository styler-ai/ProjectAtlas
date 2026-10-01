//! Execute the fixed PDF parser with bounded memory and resumable instruction fuel.

use super::limits::Failure;
use super::{
    DocumentExtractionError, DocumentFormat, DocumentLimit, MAX_DOCUMENT_EXPANDED_BYTES,
    MAX_DOCUMENT_FACTS, MAX_DOCUMENT_OUTPUT_BYTES,
};
use projectatlas_core::{IndexWorkControl, IndexWorkStage};
use std::sync::OnceLock;
use std::time::Duration;
use wasmi::errors::{MemoryError, TableError};
use wasmi::{
    Caller, Config, Engine, Instance, Linker, Module, ResourceLimiter, Store, StoreLimits,
    StoreLimitsBuilder, TypedResumableCall,
};
use wasmi_core::LimiterError;

/// Only this build-owned module can execute; callers cannot supply guest code.
const PARSER: &[u8] = include_bytes!("../../../../packaging/pdf-parser/parser.wasm");
/// Guest heap, input, decoded objects, text and wire all share this ceiling.
const MEMORY_BYTES: usize = 64 * 1024 * 1024;
/// Includes allocation, parser execution, and output accessor calls.
const TOTAL_FUEL: u64 = 500_000_000;
/// Yield often enough to observe the caller's cancellation during parsing.
const FUEL_SLICE: u64 = 10_000;

/// Keep hard allocation traps while allowing fuel exhaustion to suspend growth.
struct PdfStoreLimits {
    /// Configured Wasmi resource ceilings.
    limits: StoreLimits,
    /// Set only after the guest's structural admission pass.
    admitted: bool,
}

impl Default for PdfStoreLimits {
    fn default() -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_BYTES)
                .memories(1)
                .instances(1)
                .tables(1)
                .table_elements(4096)
                .trap_on_grow_failure(true)
                .build(),
            admitted: false,
        }
    }
}

impl ResourceLimiter for PdfStoreLimits {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, LimiterError> {
        self.limits.memory_growing(current, desired, maximum)
    }

    fn memory_grow_failed(&mut self, error: &MemoryError) -> Result<(), LimiterError> {
        // Wasmi reports allocation-side fuel exhaustion through this hook before resuming.
        if matches!(error, MemoryError::OutOfFuel { .. }) {
            return Ok(());
        }
        self.limits.memory_grow_failed(error)
    }

    fn table_growing(
        &mut self,
        current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> Result<bool, LimiterError> {
        self.limits.table_growing(current, desired, maximum)
    }

    fn table_grow_failed(&mut self, error: &TableError) -> Result<(), LimiterError> {
        if matches!(error, TableError::OutOfFuel { .. }) {
            return Ok(());
        }
        self.limits.table_grow_failed(error)
    }

    fn instances(&self) -> usize {
        self.limits.instances()
    }
    fn memories(&self) -> usize {
        self.limits.memories()
    }
    fn tables(&self) -> usize {
        self.limits.tables()
    }
}

/// Wrap a bounded parser or ABI diagnostic.
fn malformed(message: impl Into<String>) -> DocumentExtractionError {
    DocumentExtractionError::Malformed {
        format: DocumentFormat::Pdf,
        message: message.into(),
    }
}

/// Report the first value beyond an enforced ceiling.
fn limit(resource: DocumentLimit, maximum: usize) -> DocumentExtractionError {
    DocumentExtractionError::ResourceLimit {
        limit: resource,
        observed: maximum.saturating_add(1),
        maximum,
    }
}

/// Preserve interpreter resource refusals as typed document limits.
fn vm_error(error: &wasmi::Error) -> DocumentExtractionError {
    match error.as_trap_code() {
        Some(wasmi::TrapCode::GrowthOperationLimited) => {
            limit(DocumentLimit::MemoryBytes, MEMORY_BYTES)
        }
        Some(wasmi::TrapCode::StackOverflow) => malformed("PDF guest stack overflow"),
        Some(wasmi::TrapCode::OutOfFuel) => {
            limit(DocumentLimit::ExecutionFuel, TOTAL_FUEL as usize)
        }
        _ => malformed(error.to_string()),
    }
}

/// Expose entropy and an empty environment, without filesystem/network/process access.
fn linker(engine: &Engine) -> Result<Linker<PdfStoreLimits>, DocumentExtractionError> {
    let mut linker = Linker::new(engine);
    linker
        .func_wrap(
            "projectatlas",
            "admitted",
            |mut caller: Caller<'_, PdfStoreLimits>| {
                caller.data_mut().admitted = true;
            },
        )
        .map_err(|error| malformed(error.to_string()))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "random_get",
            |mut caller: Caller<'_, PdfStoreLimits>,
             ptr: u32,
             len: u32|
             -> Result<u32, wasmi::Error> {
                if len > 256 {
                    return Err(wasmi::Error::new("PDF entropy request exceeds bound"));
                }
                let memory = caller
                    .get_export("memory")
                    .and_then(wasmi::Extern::into_memory)
                    .ok_or_else(|| wasmi::Error::new("PDF memory missing"))?;
                let start = ptr as usize;
                let end = start
                    .checked_add(len as usize)
                    .ok_or_else(|| wasmi::Error::new("PDF entropy range overflow"))?;
                let bytes = memory
                    .data_mut(&mut caller)
                    .get_mut(start..end)
                    .ok_or_else(|| wasmi::Error::new("PDF entropy range invalid"))?;
                getrandom::fill(bytes).map_err(|error| wasmi::Error::new(error.to_string()))?;
                Ok(0)
            },
        )
        .map_err(|error| malformed(error.to_string()))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_sizes_get",
            |mut caller: Caller<'_, PdfStoreLimits>,
             count: u32,
             size: u32|
             -> Result<u32, wasmi::Error> {
                let memory = caller
                    .get_export("memory")
                    .and_then(wasmi::Extern::into_memory)
                    .ok_or_else(|| wasmi::Error::new("PDF memory missing"))?;
                for ptr in [count, size] {
                    memory
                        .write(&mut caller, ptr as usize, &0_u32.to_le_bytes())
                        .map_err(|error| wasmi::Error::new(error.to_string()))?;
                }
                Ok(0)
            },
        )
        .map_err(|error| malformed(error.to_string()))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_get",
            |_count: u32, _size: u32| -> u32 { 0 },
        )
        .map_err(|error| malformed(error.to_string()))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_write",
            |_fd: u32, _iov: u32, _count: u32, _written: u32| -> Result<u32, wasmi::Error> {
                Err(wasmi::Error::new("PDF guest output is denied"))
            },
        )
        .map_err(|error| malformed(error.to_string()))?;
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "proc_exit",
            |_status: u32| -> Result<(), wasmi::Error> {
                Err(wasmi::Error::new("PDF guest terminated"))
            },
        )
        .map_err(|error| malformed(error.to_string()))?;
    Ok(linker)
}

/// Translate trusted build-owned code once; each document gets an isolated store.
fn parser_module() -> Result<&'static Module, DocumentExtractionError> {
    static MODULE: OnceLock<Result<Module, String>> = OnceLock::new();
    MODULE
        .get_or_init(|| {
            let mut config = Config::default();
            config.consume_fuel(true);
            // Lazy translation can exhaust fuel outside the resumable execution boundary.
            config.compilation_mode(wasmi::CompilationMode::Eager);
            config.set_max_stack_height(1024 * 1024);
            config.set_max_recursion_depth(256);
            config.set_max_cached_stacks(0);
            let engine = Engine::new(&config);
            Module::new(&engine, PARSER).map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| malformed(error.clone()))
}

/// The only recoverable failures follow the guest's complete admission pass.
pub(super) enum PdfPages {
    /// Every page produced validated text.
    Complete(Vec<(u32, String)>),
    /// An admitted document stopped at a finite resource ceiling.
    Incomplete(DocumentLimit),
}

/// Run one document and return validated page text only after complete success.
pub(super) fn extract_pages(
    bytes: &[u8],
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<PdfPages, DocumentExtractionError> {
    let control = control.with_timeout_ceiling(
        control
            .started_at()
            .elapsed()
            .saturating_add(Duration::from_secs(10)),
    );
    control.check(stage)?;
    let module = parser_module()?;
    let engine = module.engine();
    control.check(stage)?;
    let mut store = Store::new(engine, PdfStoreLimits::default());
    store.limiter(|limits| limits);
    store
        .set_fuel(TOTAL_FUEL)
        .map_err(|error| vm_error(&error))?;
    let instance = linker(engine)?
        .instantiate_and_start(&mut store, module)
        .map_err(|error| vm_error(&error))?;
    let input = instance
        .get_typed_func::<u32, u32>(&store, "input")
        .map_err(|error| vm_error(&error))?;
    let size = u32::try_from(bytes.len()).map_err(|error| malformed(error.to_string()))?;
    let ptr = input
        .call(&mut store, size)
        .map_err(|error| vm_error(&error))?;
    if ptr == 0 {
        return Err(malformed("PDF guest refused input"));
    }
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| malformed("PDF memory missing"))?;
    memory
        .write(&mut store, ptr as usize, bytes)
        .map_err(|error| malformed(error.to_string()))?;
    control.check(stage)?;
    let extract = instance
        .get_typed_func::<(), i32>(&store, "extract")
        .map_err(|error| vm_error(&error))?;
    let outcome = run_parser(&mut store, extract, || {
        control.check(stage).map_err(DocumentExtractionError::from)
    })?;
    let status = match outcome {
        ParserRun::Complete(status) => status,
        ParserRun::FuelExhausted => return Ok(PdfPages::Incomplete(DocumentLimit::ExecutionFuel)),
        ParserRun::MemoryExhausted => return Ok(PdfPages::Incomplete(DocumentLimit::MemoryBytes)),
    };
    match Failure::try_from(status) {
        Ok(Failure::Encrypted) => return Err(DocumentExtractionError::EncryptedPdf),
        Ok(Failure::Unsupported) => return Err(DocumentExtractionError::UnsupportedPdfInput),
        Ok(Failure::Pages) => return Err(limit(DocumentLimit::FactCount, MAX_DOCUMENT_FACTS)),
        Ok(Failure::Output) if store.data().admitted => {
            return Ok(PdfPages::Incomplete(DocumentLimit::OutputBytes));
        }
        Ok(Failure::Output) => {
            return Err(limit(DocumentLimit::OutputBytes, MAX_DOCUMENT_OUTPUT_BYTES));
        }
        Ok(Failure::Expanded) => {
            return Err(limit(
                DocumentLimit::ExpandedBytes,
                MAX_DOCUMENT_EXPANDED_BYTES,
            ));
        }
        Ok(Failure::Malformed) => {
            return Err(malformed(
                "PDF parser rejected document structure or content",
            ));
        }
        Err(value) if value >= 0 => {}
        Err(_) => return Err(malformed("PDF guest returned an unknown status")),
    }
    finish_output(instance, &mut store, status as usize, &control, stage)
}

/// Finish an admitted parse without treating accessor fuel as a document failure.
fn finish_output(
    instance: Instance,
    store: &mut Store<PdfStoreLimits>,
    expected_bytes: usize,
    control: &IndexWorkControl,
    stage: IndexWorkStage,
) -> Result<PdfPages, DocumentExtractionError> {
    let pages = match read_output(instance, store, expected_bytes) {
        Ok(pages) => pages,
        Err(DocumentExtractionError::ResourceLimit {
            limit: DocumentLimit::ExecutionFuel,
            ..
        }) if store.data().admitted => {
            control.check(stage)?;
            return Ok(PdfPages::Incomplete(DocumentLimit::ExecutionFuel));
        }
        Err(error) => return Err(error),
    };
    control.check(stage)?;
    Ok(PdfPages::Complete(pages))
}

/// One resumable guest call before output access.
#[derive(Debug)]
enum ParserRun {
    /// Guest returned its status code.
    Complete(i32),
    /// Finite interpreter fuel ended after admission.
    FuelExhausted,
    /// Configured linear memory ended after admission.
    MemoryExhausted,
}

/// Spend a single finite fuel balance, checking cancellation at each suspension.
fn run_parser(
    store: &mut Store<PdfStoreLimits>,
    extract: wasmi::TypedFunc<(), i32>,
    mut check: impl FnMut() -> Result<(), DocumentExtractionError>,
) -> Result<ParserRun, DocumentExtractionError> {
    let mut remaining = store.get_fuel().map_err(|error| vm_error(&error))?;
    let maximum = usize::try_from(remaining).map_err(|error| malformed(error.to_string()))?;
    let mut grant = FUEL_SLICE.min(remaining);
    store.set_fuel(grant).map_err(|error| vm_error(&error))?;
    let mut state = match extract.call_resumable(&mut *store, ()) {
        Ok(state) => state,
        Err(error)
            if store.data().admitted
                && error.as_trap_code() == Some(wasmi::TrapCode::GrowthOperationLimited) =>
        {
            check()?;
            return Ok(ParserRun::MemoryExhausted);
        }
        Err(error) => return Err(vm_error(&error)),
    };
    let status = loop {
        remaining -= grant - store.get_fuel().map_err(|error| vm_error(&error))?;
        check()?;
        match state {
            TypedResumableCall::Finished(status) => break status,
            TypedResumableCall::OutOfFuel(call) => {
                let needed = call.required_fuel();
                if remaining == 0 || needed > remaining {
                    return if store.data().admitted {
                        Ok(ParserRun::FuelExhausted)
                    } else {
                        Err(limit(DocumentLimit::ExecutionFuel, maximum))
                    };
                }
                grant = FUEL_SLICE.max(needed).min(remaining);
                store.set_fuel(grant).map_err(|error| vm_error(&error))?;
                state = match call.resume(&mut *store) {
                    Ok(state) => state,
                    Err(error)
                        if store.data().admitted
                            && error.as_trap_code()
                                == Some(wasmi::TrapCode::GrowthOperationLimited) =>
                    {
                        check()?;
                        return Ok(ParserRun::MemoryExhausted);
                    }
                    Err(error) => return Err(vm_error(&error)),
                };
            }
            TypedResumableCall::HostTrap(_) => {
                return Err(malformed("PDF guest attempted a denied host operation"));
            }
        }
    };
    store
        .set_fuel(remaining)
        .map_err(|error| vm_error(&error))?;
    Ok(ParserRun::Complete(status))
}

/// Validate all guest records before copying text into native ownership.
fn read_output(
    instance: Instance,
    store: &mut Store<PdfStoreLimits>,
    expected_bytes: usize,
) -> Result<Vec<(u32, String)>, DocumentExtractionError> {
    let ptr = instance
        .get_typed_func::<(), u32>(&*store, "output_ptr")
        .map_err(|error| vm_error(&error))?
        .call(&mut *store, ())
        .map_err(|error| vm_error(&error))? as usize;
    let len = instance
        .get_typed_func::<(), u32>(&*store, "output_len")
        .map_err(|error| vm_error(&error))?
        .call(&mut *store, ())
        .map_err(|error| vm_error(&error))? as usize;
    if len > MAX_DOCUMENT_OUTPUT_BYTES + MAX_DOCUMENT_FACTS * 8 + 4 {
        return Err(limit(DocumentLimit::OutputBytes, MAX_DOCUMENT_OUTPUT_BYTES));
    }
    let end = ptr
        .checked_add(len)
        .ok_or_else(|| malformed("PDF output range overflow"))?;
    let memory = instance
        .get_memory(&*store, "memory")
        .ok_or_else(|| malformed("PDF memory missing"))?;
    let mut wire = memory
        .data(&*store)
        .get(ptr..end)
        .ok_or_else(|| malformed("PDF output range invalid"))?;
    let count = word(&mut wire)? as usize;
    if count > MAX_DOCUMENT_FACTS {
        return Err(malformed("PDF page count invalid"));
    }
    let mut pages = Vec::with_capacity(count);
    let mut total = 0usize;
    for expected in 1..=count {
        let page = word(&mut wire)?;
        if page as usize != expected {
            return Err(malformed("PDF page order invalid"));
        }
        let len = word(&mut wire)? as usize;
        total = total.saturating_add(len);
        if total > MAX_DOCUMENT_OUTPUT_BYTES {
            return Err(limit(DocumentLimit::OutputBytes, MAX_DOCUMENT_OUTPUT_BYTES));
        }
        let (text, tail) = wire
            .split_at_checked(len)
            .ok_or_else(|| malformed("PDF page text truncated"))?;
        pages.push((
            page,
            std::str::from_utf8(text)
                .map_err(|error| malformed(error.to_string()))?
                .to_owned(),
        ));
        wire = tail;
    }
    if !wire.is_empty() || total != expected_bytes {
        return Err(malformed("PDF output length invalid"));
    }
    Ok(pages)
}

/// Consume one checked little-endian wire word.
fn word(wire: &mut &[u8]) -> Result<u32, DocumentExtractionError> {
    let (bytes, tail) = wire
        .split_at_checked(4)
        .ok_or_else(|| malformed("PDF output word truncated"))?;
    *wire = tail;
    Ok(u32::from_le_bytes(
        <[u8; 4]>::try_from(bytes).map_err(|error| malformed(error.to_string()))?,
    ))
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use projectatlas_core::{IndexCancellation, IndexWorkFailure};

    fn guest_with_input(
        bytes: &[u8],
    ) -> (Store<PdfStoreLimits>, Instance, wasmi::TypedFunc<(), i32>) {
        let module = parser_module().expect("fixed module");
        let mut store = Store::new(module.engine(), PdfStoreLimits::default());
        store.limiter(|limits| limits);
        store.set_fuel(TOTAL_FUEL).unwrap();
        let instance = linker(module.engine())
            .unwrap()
            .instantiate_and_start(&mut store, module)
            .unwrap();
        let input = instance
            .get_typed_func::<u32, u32>(&store, "input")
            .unwrap();
        let ptr = input
            .call(&mut store, u32::try_from(bytes.len()).unwrap())
            .unwrap();
        instance
            .get_memory(&store, "memory")
            .unwrap()
            .write(&mut store, ptr as usize, bytes)
            .unwrap();
        let extract = instance
            .get_typed_func::<(), i32>(&store, "extract")
            .unwrap();
        (store, instance, extract)
    }

    fn admitted_guest() -> (Store<PdfStoreLimits>, Instance, wasmi::TypedFunc<(), i32>) {
        guest_with_input(&super::super::tests::minimal_pdf())
    }

    #[test]
    fn malformed_later_page_never_sets_admission_marker() {
        let mut document =
            lopdf::Document::load_mem(&super::super::tests::multi_page_pdf()).unwrap();
        document
            .get_object_mut((7, 0))
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"(".to_vec());
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let (mut store, _, extract) = guest_with_input(&bytes);
        let result = run_parser(&mut store, extract, || Ok(()));
        assert!(
            matches!(result, Ok(ParserRun::Complete(code)) if code == Failure::Malformed as i32)
        );
        assert!(!store.data().admitted);
    }

    #[test]
    fn admitted_output_accessor_fuel_is_a_local_gap() {
        let (mut store, instance, extract) = admitted_guest();
        let result = run_parser(&mut store, extract, || Ok(())).unwrap();
        assert!(matches!(result, ParserRun::Complete(_)));
        let ParserRun::Complete(status) = result else {
            return;
        };
        assert!(store.data().admitted);
        store.set_fuel(0).unwrap();
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let result = finish_output(
            instance,
            &mut store,
            status as usize,
            &control,
            IndexWorkStage::TextIndex,
        );
        assert!(matches!(
            result,
            Ok(PdfPages::Incomplete(DocumentLimit::ExecutionFuel))
        ));
        cancellation.cancel();
        let result = finish_output(
            instance,
            &mut store,
            status as usize,
            &control,
            IndexWorkStage::TextIndex,
        );
        assert!(matches!(
            result,
            Err(DocumentExtractionError::Work(
                IndexWorkFailure::Cancelled { .. }
            ))
        ));
    }

    #[test]
    fn resumptions_cannot_refill_the_total_execution_budget() {
        let (mut store, _, extract) = admitted_guest();
        store.set_fuel(FUEL_SLICE * 2).unwrap();
        let mut checkpoints = 0;
        let result = run_parser(&mut store, extract, || {
            checkpoints += 1;
            Ok(())
        });
        assert!(
            matches!(
                result,
                Err(DocumentExtractionError::ResourceLimit {
                    limit: DocumentLimit::ExecutionFuel,
                    ..
                })
            ),
            "{result:?}"
        );
        assert!(
            checkpoints >= 2,
            "must resume actual parsing before exhausting total fuel"
        );
    }

    #[test]
    fn cancellation_stops_a_suspended_parse_without_returning_text() {
        let (mut store, _, extract) = admitted_guest();
        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), None);
        let mut checkpoints = 0;
        let result = run_parser(&mut store, extract, || {
            checkpoints += 1;
            if checkpoints == 2 {
                cancellation.cancel();
            }
            control
                .check(IndexWorkStage::TextIndex)
                .map_err(DocumentExtractionError::from)
        });
        assert!(matches!(
            result,
            Err(DocumentExtractionError::Work(IndexWorkFailure::Cancelled {
                stage: IndexWorkStage::TextIndex
            }))
        ));
        assert_eq!(checkpoints, 2);
        assert!(store.get_fuel().unwrap() < FUEL_SLICE);
    }

    #[test]
    fn deadline_stops_a_suspended_parse_without_returning_text() {
        let (mut store, _, extract) = admitted_guest();
        let mut checkpoints = 0;
        let mut control = IndexWorkControl::new(IndexCancellation::new(), None);
        let result = run_parser(&mut store, extract, || {
            checkpoints += 1;
            if checkpoints == 2 {
                control = IndexWorkControl::with_deadline(
                    IndexCancellation::new(),
                    std::time::Instant::now(),
                );
            }
            control
                .check(IndexWorkStage::TextIndex)
                .map_err(DocumentExtractionError::from)
        });
        assert!(matches!(
            result,
            Err(DocumentExtractionError::Work(
                IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::TextIndex
                }
            ))
        ));
        assert_eq!(checkpoints, 2);
    }

    #[test]
    fn actual_guest_memory_growth_keeps_the_hard_allocation_limit() {
        let module = parser_module().unwrap();
        let engine = module.engine();
        // () -> i32, one memory, grow by 1152 pages (72 MiB).
        let wasm = b"\0asm\x01\0\0\0\x01\x05\x01\x60\x00\x01\x7f\x03\x02\x01\x00\x05\x03\x01\x00\x01\x07\x0a\x01\x06invoke\x00\x00\x0a\x09\x01\x07\x00\x41\x80\x09\x40\x00\x0b";
        let module = Module::new(engine, wasm).unwrap();
        for admitted in [false, true] {
            let mut store = Store::new(engine, PdfStoreLimits::default());
            store.limiter(|limits| limits);
            store.set_fuel(TOTAL_FUEL).unwrap();
            let instance = linker(engine)
                .unwrap()
                .instantiate_and_start(&mut store, &module)
                .unwrap();
            store.data_mut().admitted = admitted;
            let grow = instance
                .get_typed_func::<(), i32>(&store, "invoke")
                .unwrap();
            let result = run_parser(&mut store, grow, || Ok(()));
            if admitted {
                assert!(
                    matches!(result, Ok(ParserRun::MemoryExhausted)),
                    "{result:?}"
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(DocumentExtractionError::ResourceLimit {
                            limit: DocumentLimit::MemoryBytes,
                            ..
                        })
                    ),
                    "{result:?}"
                );
            }
        }
    }

    #[test]
    fn stack_overflow_is_fatal_not_an_accepted_linear_memory_gap() {
        let error = wasmi::Error::from(wasmi::TrapCode::StackOverflow);
        assert!(matches!(
            vm_error(&error),
            DocumentExtractionError::Malformed { .. }
        ));
    }

    #[test]
    fn admitted_memory_stop_does_not_mask_cancellation_or_deadline() {
        let engine = parser_module().unwrap().engine();
        let wasm = b"\0asm\x01\0\0\0\x01\x05\x01\x60\x00\x01\x7f\x03\x02\x01\x00\x05\x03\x01\x00\x01\x07\x0a\x01\x06invoke\x00\x00\x0a\x09\x01\x07\x00\x41\x80\x09\x40\x00\x0b";
        let module = Module::new(engine, wasm).unwrap();
        for cancelled in [true, false] {
            let mut store = Store::new(engine, PdfStoreLimits::default());
            store.limiter(|limits| limits);
            store.set_fuel(TOTAL_FUEL).unwrap();
            let instance = linker(engine)
                .unwrap()
                .instantiate_and_start(&mut store, &module)
                .unwrap();
            store.data_mut().admitted = true;
            let grow = instance
                .get_typed_func::<(), i32>(&store, "invoke")
                .unwrap();
            let result = run_parser(&mut store, grow, || {
                Err(DocumentExtractionError::Work(if cancelled {
                    IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::TextIndex,
                    }
                } else {
                    IndexWorkFailure::DeadlineExceeded {
                        stage: IndexWorkStage::TextIndex,
                    }
                }))
            });
            if cancelled {
                assert!(matches!(
                    result,
                    Err(DocumentExtractionError::Work(
                        IndexWorkFailure::Cancelled { .. }
                    ))
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(DocumentExtractionError::Work(
                        IndexWorkFailure::DeadlineExceeded { .. }
                    ))
                ));
            }
        }
    }

    // Tiny binary fixture: one imported function re-exported as `invoke`.
    // All section and string lengths fit one unsigned LEB128 byte.
    fn import_fixture(engine: &Engine, name: &str, params: u8, results: u8) -> Module {
        let mut wasm = b"\0asm\x01\0\0\0".to_vec();
        let mut ty = vec![1, 0x60, params];
        ty.extend(std::iter::repeat_n(0x7f, usize::from(params)));
        ty.push(results);
        ty.extend(std::iter::repeat_n(0x7f, usize::from(results)));
        let namespace = b"wasi_snapshot_preview1";
        let mut import = vec![1, u8::try_from(namespace.len()).unwrap()];
        import.extend(namespace);
        import.push(u8::try_from(name.len()).unwrap());
        import.extend(name.as_bytes());
        import.extend([0, 0]);
        let export = b"\x01\x06invoke\x00\x00".to_vec();
        for (id, section) in [(1, ty), (2, import), (7, export)] {
            assert!(section.len() < 128);
            wasm.extend([id, u8::try_from(section.len()).unwrap()]);
            wasm.extend(section);
        }
        Module::new(engine, wasm).unwrap()
    }

    #[test]
    fn host_cannot_supply_filesystem_network_clock_or_output_capabilities() {
        let engine = Engine::default();
        let linker = linker(&engine).unwrap();
        for name in ["path_open", "sock_open", "clock_time_get"] {
            let module = import_fixture(&engine, name, 0, 0);
            let mut store = Store::new(&engine, PdfStoreLimits::default());
            assert!(
                linker.instantiate_and_start(&mut store, &module).is_err(),
                "{name} must remain unavailable"
            );
        }
        let mut store = Store::new(&engine, PdfStoreLimits::default());
        let module = import_fixture(&engine, "fd_write", 4, 1);
        let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
        let write = instance
            .get_typed_func::<(u32, u32, u32, u32), u32>(&store, "invoke")
            .unwrap();
        assert!(write.call(&mut store, (1, 0, 0, 0)).is_err());
        let module = import_fixture(&engine, "proc_exit", 1, 0);
        let instance = linker.instantiate_and_start(&mut store, &module).unwrap();
        let exit = instance
            .get_typed_func::<u32, ()>(&store, "invoke")
            .unwrap();
        assert!(exit.call(&mut store, 0).is_err());
    }
}
