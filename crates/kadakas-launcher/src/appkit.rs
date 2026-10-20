//! The macOS side of `events`: the open-documents Apple event (PLAT-16).
//!
//! A double click on a `.kad` file makes Launch Services start the program of the outer bundle
//! (this launcher) and send it the Apple event `odoc`, which AppKit turns into
//! `application:openURLs:` on the application delegate. AppKit sends it before
//! `applicationDidFinishLaunching:`. While the launcher runs, later double clicks (and `open -a
//! Kadakas file.kad`) go to the same process, so the delegate sees them too.
//!
//! The launcher has no window and no Dock icon (`LSUIElement` in its `Info.plist`; the activation
//! policy is also set to accessory, for a launcher that runs outside a bundle). This file is thin
//! on purpose: it only turns AppKit calls into [`EventHandler`] calls. The logic is in `events`.

use crate::events::{EventHandler, SystemEvents};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
    NSApplicationDelegate, NSRunningApplication,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSURL,
};
use std::path::PathBuf;
use std::sync::Arc;

struct Ivars {
    handler: Arc<dyn EventHandler>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Delegate` does not implement Drop.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Delegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSApplicationDelegate for Delegate {
        // SAFETY: The signature is correct.
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _notification: &NSNotification) {
            self.ivars().handler.ready();
        }

        // SAFETY: The signature is correct.
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            let files: Vec<PathBuf> = urls
                .iter()
                .filter(|u| u.isFileURL())
                .filter_map(|u| u.path())
                .map(|p| PathBuf::from(p.to_string()))
                .collect();
            if !files.is_empty() {
                self.ivars().handler.open(files);
            }
        }

        // SAFETY: The signature is correct.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            self.ivars().handler.reopen();
            // Nothing else to do: the launcher has no window.
            false
        }
    }
);

impl Delegate {
    fn new(mtm: MainThreadMarker, handler: Arc<dyn EventHandler>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars { handler });
        // SAFETY: The signature of `init` on NSObject is correct.
        unsafe { msg_send![super(this), init] }
    }
}

unsafe extern "C" {
    static _dispatch_main_q: std::ffi::c_void;
    fn dispatch_async_f(
        queue: *const std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

/// The real system events.
pub struct AppKitEvents;

impl SystemEvents for AppKitEvents {
    fn run(&self, handler: Arc<dyn EventHandler>) {
        let Some(mtm) = MainThreadMarker::new() else {
            eprintln!("kadakas-launcher: the event loop must run on the main thread");
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        // No Dock icon and no menu bar: the app has its own.
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let delegate = Delegate::new(mtm, handler);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.run();
        // `run` returns only if something stops the loop; keep the delegate until then.
        drop(delegate);
    }

    fn stop(&self, success: bool) {
        // Called from the worker thread. AppKit wants its calls on the main thread, so the
        // termination goes there (a block on the main queue, run by the event loop).
        extern "C" fn terminate(context: *mut std::ffi::c_void) {
            let success = !context.is_null();
            if !success {
                // `terminate:` ends the process with status 0. A failed start must say 1.
                std::process::exit(1);
            }
            if let Some(mtm) = MainThreadMarker::new() {
                NSApplication::sharedApplication(mtm).terminate(None);
            }
            std::process::exit(0);
        }
        // SAFETY: `_dispatch_main_q` is the main queue of libdispatch, `dispatch_async_f` takes a
        // plain function and a context pointer that it passes on unchanged (here a flag, not a
        // pointer to memory).
        unsafe {
            dispatch_async_f(
                std::ptr::addr_of!(_dispatch_main_q),
                if success {
                    std::ptr::dangling_mut()
                } else {
                    std::ptr::null_mut()
                },
                terminate,
            );
        }
    }

    fn activate(&self, pid: u32) {
        let Ok(pid) = i32::try_from(pid) else {
            return;
        };
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            // Deprecated in macOS 14 for the new cooperative activation, which needs the caller
            // to be active. The launcher never is, so this call is still the one that works.
            #[allow(deprecated)]
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
    }
}
