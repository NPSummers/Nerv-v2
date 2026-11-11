use super::{closure::Closure, standard::strings, *};
use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64},
    },
    thread::{self, JoinHandle},
};

struct Task {
    cancel: Arc<AtomicBool>,
    worker: JoinHandle<i64>,
    depth: usize,
}

thread_local! {
    static TASKS: RefCell<HashMap<i64, Task>> = RefCell::new(HashMap::new());
    static CANCELLATION: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}
static NEXT_TASK: AtomicI64 = AtomicI64::new(1);

type Worker = unsafe extern "C" fn(*mut c_void, i64) -> i64;

unsafe fn worker(callback: *const Closure) -> Result<Worker, String> {
    let callback = unsafe { callback.as_ref() }.ok_or("invalid task callback")?;
    if !callback.environment.is_null() {
        return Err("task and region callbacks must not capture outer values".into());
    }
    Ok(unsafe { std::mem::transmute::<*const c_void, Worker>(callback.code) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_memory_scope(input: i64, callback: *const Closure) -> i64 {
    let callback = unsafe { worker(callback) }.unwrap_or_else(|error| runtime_error(&error));
    let _execution = Execution::new();
    unsafe { callback(std::ptr::null_mut(), input) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_task_spawn(input: i64, callback: *const Closure) -> *mut c_void {
    let result = (|| {
        let callback = unsafe { worker(callback) }?;
        let id = NEXT_TASK.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let handle = thread::Builder::new()
            .name(format!("nerv-task-{id}"))
            .spawn(move || {
                CANCELLATION.with(|flag| *flag.borrow_mut() = Some(worker_cancel));
                let _execution = Execution::new();
                unsafe { callback(std::ptr::null_mut(), input) }
            })
            .map_err(|error| error.to_string())?;
        let depth = ALLOCATIONS.with(|regions| regions.borrow().len());
        TASKS.with(|tasks| {
            tasks.borrow_mut().insert(
                id,
                Task {
                    cancel,
                    worker: handle,
                    depth,
                },
            )
        });
        Ok::<_, String>(id)
    })();
    match result {
        Ok(id) => strings(vec![id.to_string(), String::new()]),
        Err(error) => strings(vec![String::new(), error]),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_task_cancel(id: i64) -> i64 {
    TASKS.with(|tasks| match tasks.borrow().get(&id) {
        Some(task) => {
            task.cancel.store(true, Ordering::Release);
            1
        }
        None => 0,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_task_cancelled() -> i64 {
    CANCELLATION.with(|flag| {
        i64::from(
            flag.borrow()
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire)),
        )
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_task_join(id: i64) -> *mut c_void {
    let task = TASKS.with(|tasks| tasks.borrow_mut().remove(&id));
    match task {
        Some(task) => match task.worker.join() {
            Ok(value) => strings(vec![value.to_string(), String::new()]),
            Err(_) => strings(vec![String::new(), "task panicked".into()]),
        },
        None => strings(vec![
            String::new(),
            "task handle is invalid or already joined".into(),
        ]),
    }
}

pub(super) fn finish_region(depth: usize) {
    let tasks = TASKS.with(|tasks| {
        let mut tasks = tasks.borrow_mut();
        let ids = tasks
            .iter()
            .filter(|(_, task)| task.depth == depth)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        ids.into_iter()
            .map(|id| tasks.remove(&id).unwrap())
            .collect::<Vec<_>>()
    });
    for task in &tasks {
        task.cancel.store(true, Ordering::Release);
    }
    for task in tasks {
        let _ = task.worker.join();
    }
}

pub(super) fn symbols() -> Vec<RuntimeSymbol> {
    vec![
        RuntimeSymbol {
            name: "nerv_memory_scope",
            address: nerv_memory_scope as *const (),
        },
        RuntimeSymbol {
            name: "nerv_task_spawn",
            address: nerv_task_spawn as *const (),
        },
        RuntimeSymbol {
            name: "nerv_task_cancel",
            address: nerv_task_cancel as *const (),
        },
        RuntimeSymbol {
            name: "nerv_task_cancelled",
            address: nerv_task_cancelled as *const (),
        },
        RuntimeSymbol {
            name: "nerv_task_join",
            address: nerv_task_join as *const (),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn temporary(_: *mut c_void, input: i64) -> i64 {
        for _ in 0..100 {
            string("temporary".repeat(100));
        }
        input + 1
    }

    unsafe extern "C" fn cancellable(_: *mut c_void, input: i64) -> i64 {
        while nerv_task_cancelled() == 0 {
            thread::yield_now();
        }
        input
    }

    #[test]
    fn allocation_scopes_reclaim_temporary_values() {
        let _outer = Execution::new();
        let callback = Closure {
            code: temporary as *const c_void,
            environment: std::ptr::null_mut(),
            mutable_captures: 0,
        };
        let before = active_region_allocations();
        for i in 0..100 {
            assert_eq!(unsafe { nerv_memory_scope(i, &callback) }, i + 1);
            assert_eq!(active_region_allocations(), before);
        }
    }

    #[test]
    fn region_exit_cancels_and_joins_unfinished_tasks() {
        let callback = Closure {
            code: cancellable as *const c_void,
            environment: std::ptr::null_mut(),
            mutable_captures: 0,
        };
        {
            let _execution = Execution::new();
            unsafe {
                nerv_task_spawn(42, &callback);
            }
            assert_eq!(TASKS.with(|tasks| tasks.borrow().len()), 1);
        }
        assert!(TASKS.with(|tasks| tasks.borrow().is_empty()));
    }

    #[test]
    fn captured_task_callbacks_are_rejected() {
        let mut data = 1i64;
        let callback = Closure {
            code: temporary as *const c_void,
            environment: (&mut data as *mut i64).cast(),
            mutable_captures: 0,
        };
        assert!(unsafe { worker(&callback) }.is_err());
    }
}
