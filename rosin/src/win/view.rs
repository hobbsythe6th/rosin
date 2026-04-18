
use std::{any::Any, time::Duration, collections::VecDeque, sync::Mutex};

use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Direct2D::Common::D2D_SIZE_U;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetDesktopWindow, GetWindowRect, GetWindowThreadProcessId, HMENU, SW_NORMAL, ShowWindowAsync};
use windows::core::Error;

use crate::{
    kurbo::{Point, Size},
    prelude::*,
};

use crate::desc::WindowDesc;

// these 2 are separate to not drop the vec within the same fn,
// to not cause a mem leak and to break apart the "expensive part" and the "cheep part"

/// Converts a utf8 str to a utf16 vector
/// 
/// Allocates and copies the data
pub(crate) fn utf8_to_utf16(string: &str) -> Vec<u16> {
    let os_str = AsRef::<std::ffi::OsStr>::as_ref(string);

    std::os::windows::ffi::OsStrExt::encode_wide(os_str)
        .chain(std::iter::once(0))
        .collect()
}

/// Casts a rust utf16 slice to a win32 utf16 string
/// 
/// Is close to a noop
pub(crate) fn utf16_as_pcwstr(slice: &[u16]) -> windows::core::PCWSTR {
    windows::core::PCWSTR::from_raw(slice.as_ptr())
}

/// A struct for safely locking the use of a View on a single thread
pub(crate) struct ThreadLockedView {
    view: RosinView,
    thread_id: u32,

    action_queue: Mutex<VecDeque<Box<dyn FnOnce(&RosinView)>>>
}

impl ThreadLockedView {
    // this is the best that can be done without knowing the "main" thread from what I've reaserched
    // It's a vary leaky and not-good implementation but windows does not provide the api to know
    // which thread is the "main" thread without acces to the "main" thread.
    //
    // If there is a way to detect the "main" thread then it should be 110% added
    pub fn new(view: RosinView) -> ThreadLockedView {
        Self {
            thread_id: unsafe {
                // SAFETY: this is ran on a valid thread
                GetWindowThreadProcessId(view.hwnd(), None)
            },
            view,

            action_queue: Mutex::new(VecDeque::with_capacity(64)),
        }
    }

    /// Tries to execute immidietly
    ///
    /// Returns [`None`] if it's not found on the original thread
    pub fn try_on_thread<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&RosinView) -> R,
    {
        let current_id = unsafe { GetCurrentThreadId() };
        (self.thread_id == current_id).then(|| f(&self.view))
    }

    /// SAFETY: All actions performed on the given `&RosinView` must be locked to the thread `RosinView` was created on.
    pub unsafe fn try_on_trust<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&RosinView) -> R + Sync + 'static,
        R: Sync + 'static,
    {
        f(&self.view)
    }

    pub fn queue_on_thread<F>(&self, f: F)
    where
        F: FnOnce(&RosinView) + Sync + 'static,
    {
        let current_id = unsafe { GetCurrentThreadId() };

        let mut queue = self
            .action_queue
            .lock()
            .expect("Can not do anything if the `action_queue` is poisoned except *maybe* recover it?");

        if self.thread_id == current_id {
            for func in queue.drain(..) {
                func(&self.view)
            }

            f(&self.view)
        } else {
            queue.push_back(Box::new(f))
        }
    }

    pub fn block_on_thread<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&RosinView) -> R + Sync + 'static,
        R: Sync + 'static,
    {
        todo!("`block_on_thread` blocking on thread (hehe)")
    }
}

// SAFETY: You can only acces the !Send View and UnsafeCell if it's on the original thread
unsafe impl Send for ThreadLockedView {}

// SAFETY: You can only acces the !Sync View and UnsafeCell if it's on the original thread
unsafe impl Sync for ThreadLockedView {}

pub(crate) struct RosinView {
    hwnd: HWND,
}

// here to easely change the impl later
pub(crate) fn f64_to_i32(f: f64) -> i32 {
    f as i32
}

fn menu(desc: MenuDesc) -> Result<HMENU, Error> {
    use crate::menu::{
        MenuItem,

        StandardAction,
    };

    use windows::Win32::UI::WindowsAndMessaging::{
        CreateMenu,
        InsertMenuItemW,
        SC_SEPARATOR
    };

    let menu = unsafe {
        // SAFETY: No inputs => all inputs are valid (yipee)
        CreateMenu()?
    };

    // use the InsertMenuItem, AppendMenu, and InsertMenu functions

    for item in desc.items.iter() {
        let insert = match item {
            MenuItem::Action {
                title,
                command,
                shortcut,
                enabled,
                selected,
            } => {
                todo!("adding an action within a menu")
            }
            MenuItem::Submenu { title, menu, enabled } => {
                todo!("adding a submenu within a menu")
            }
            MenuItem::Standard( standard ) => {
                match standard {
                    StandardAction::Copy      => todo!("adding a standard copy action within a menu"),
                    StandardAction::Cut       => todo!("adding a standard cut action within a menu"),
                    StandardAction::Paste     => todo!("adding a standard paste action within a menu"),
                    StandardAction::SelectAll => todo!("adding a standard select all action within a menu"),
                }
            }
            MenuItem::Separator => {
                todo!("adding a separator within a menu")
            }
        };

        unsafe {
            InsertMenuItemW(
                menu,
                0 /* add here item stuff */,
                false /* add here pos stuff */,
                insert /* add here data stuff */,
            )?
        }
    }

    Ok(menu)
}

// TODO: implement all the unused (menu) stuff
impl RosinView {
    pub fn from_new_window<S: 'static>(desc: &WindowDesc<S>, instance: Option<HINSTANCE>, parent: Option<WindowHandle>) -> Result<RosinView, Error> {
        use windows::Win32::UI::WindowsAndMessaging::{
            WINDOW_STYLE,

            WS_EX_OVERLAPPEDWINDOW,
            WS_CAPTION,
            WS_SYSMENU,
            WS_SIZEBOX,
            WS_MINIMIZEBOX,
            WS_MAXIMIZEBOX,
        };
        
        println!("Initializing rosin view");

        let desktop = unsafe { GetDesktopWindow() };

        let width = f64_to_i32(desc.size.width);
        let height = f64_to_i32(desc.size.height);

        let (x, y) = 'pos: {
            if let Some(pos) = desc.position {
                break 'pos (f64_to_i32(pos.x), f64_to_i32(pos.y));
            }

            let desktop_size = {
                let mut rect = Default::default();
                unsafe {
                    // SAFETY:
                    //  - desktop is a valid window handle
                    //  - &raw mut rect is a valid RECT address
                    GetWindowRect(desktop, &raw mut rect)?;
                }
                rect
            };

            let desktop_width = desktop_size.right - desktop_size.left;
            let desktop_height = desktop_size.bottom - desktop_size.top;

            let x = (desktop_width - width) / 2;
            let y = (desktop_height - height) / 2;

            (x, y)
        };

        let view_state = Box::new(ViewState::new());

        let Ok(menu) = desc.menu.clone().map(menu).transpose() else {
            todo!("handling failure to create menu gracefully (aka returning an error)")
        };

        let window_style = {
            let mut window_style = WS_CAPTION | WS_SYSMENU;

            if !desc.close_button {
                todo!("removing the (unsuported officially) close button")
            }

            if desc.maximize_button {
                window_style = window_style | WS_MAXIMIZEBOX;
            }

            if desc.minimize_button {
                window_style = window_style | WS_MINIMIZEBOX;
            }

            if desc.resizeable {
                window_style = window_style | WS_SIZEBOX;
            }

            window_style
        };

        // I tried looking for another safe
        // or at least safer api for creating a window,
        // but for now this should hopefully do.
        let view = RosinView {
            hwnd: unsafe {
                // FIXME add safety comments
                windows::Win32::UI::WindowsAndMessaging::CreateWindowExW(
                    WS_EX_OVERLAPPEDWINDOW,
                    crate::platform::proc_fn::ROSIN_CLASS,
                    // TODO check: From my testing this "clones" the string; Not 100% sure if it's sound or UB though
                    desc.title
                        .as_deref()
                        .map(utf8_to_utf16)
                        .as_deref()
                        .map(utf16_as_pcwstr)
                        .as_ref(),
                    window_style,
                    x,
                    y,
                    width,
                    height,
                    Some(parent.map(|handle| handle.0.view.view.hwnd).unwrap_or(desktop)),
                    menu,
                    instance,
                    Some(Box::leak(view_state) as *mut _ as *const _),
                )?
            },
        };

        unsafe {
            // SAFETY:
            //  - view.hwnd is a valid handle
            //  - SW_NORMAL is a valid show window code
            ShowWindowAsync(view.hwnd, SW_NORMAL).ok()?;
        }

        Ok(view)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn set_input_handler(&self, _id: Option<NodeId>, _handler: Option<Box<dyn InputHandler + Send + Sync>>) {}

    pub fn get_logical_size(&self) -> Size {
        Size::ZERO
    }

    pub fn get_physical_size(&self) -> Size {
        Size::ZERO
    }

    pub fn get_position(&self) -> Point {
        Point::ZERO
    }

    pub fn get_window_state(&self) -> WindowState {
        WindowState::Normal
    }

    pub fn is_active(&self) -> bool {
        true
    }

    pub fn activate(&self) {}

    pub fn deactivate(&self) {}

    pub fn set_menu(&self, _menu: impl Into<Option<MenuDesc>>) {}

    pub fn show_context_menu(&self, _node: Option<NodeId>, _menu: MenuDesc, _pos: Point) {}

    pub fn create_window<S: Any + Sync + 'static>(&self, _desc: &WindowDesc<S>) {}

    pub fn request_close(&self) {}

    pub fn request_exit(&self) {}

    pub fn set_max_size(&self, _size: Option<impl Into<Size>>) {}

    pub fn set_min_size(&self, _size: Option<impl Into<Size>>) {}

    pub fn set_resizable(&self, _resizeable: bool) {}

    pub fn set_title(&self, _title: impl Into<String>) {}

    pub fn minimize(&self) {
        use windows::Win32::UI::WindowsAndMessaging::SW_MINIMIZE;

        unsafe {
            // SAFETY: all given values are valid
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindowAsync(self.hwnd(), SW_MINIMIZE);
        }
    }

    pub fn maximize(&self) {
        use windows::Win32::UI::WindowsAndMessaging::SW_MAXIMIZE;

        unsafe {
            // SAFETY: all given values are valid
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindowAsync(self.hwnd(), SW_MAXIMIZE);
        }
    }

    pub fn restore(&self) {
        use windows::Win32::UI::WindowsAndMessaging::SW_RESTORE;

        unsafe {
            // SAFETY: all given values are valid
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindowAsync(self.hwnd(), SW_RESTORE);
        }
    }

    pub fn set_cursor(&self, _cursor: CursorType) {}

    pub fn hide_cursor(&self) {}

    pub fn unhide_cursor(&self) {}

    pub fn set_clipboard_text(&self, _text: &str) {}

    pub fn get_clipboard_text(&self) -> Option<String> {
        None
    }

    pub fn open_url(&self, _url: &str) {}

    pub fn open_file_dialog(&self, _node: Option<NodeId>, _options: FileDialogOptions) {}

    pub fn save_file_dialog(&self, _node: Option<NodeId>, _options: FileDialogOptions) {}

    pub fn timer(&self, _node: Option<NodeId>, _delay: Duration) {}

    pub fn alert<C>(&self, _node: Option<NodeId>, _png_bytes: Option<&'static [u8]>, _title: &str, _details: &str, _options: &[(&'static str, C)])
    where
        C: Into<CommandId> + Copy,
    {
    }
}

impl Drop for RosinView {
    fn drop(&mut self) {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{GWLP_USERDATA, GetWindowLongPtrW};

            // SAFETY: self.hwnd is a valid handle
            debug_assert!(!self.hwnd.is_invalid(), "`hwnd` at this point should be a valid window handle");
            if let Some(view_state) = std::ptr::NonNull::new(GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut ViewState) {
                // SAFETY: view_state is initialized to be valid
                std::mem::drop(Box::from_raw(view_state.as_ptr()));
            }
        }
    }
}

use windows::Win32::Graphics::Direct2D::{D2D1_FACTORY_TYPE_MULTI_THREADED, D2D1CreateFactory, ID2D1Factory8, ID2D1HwndRenderTarget};

#[repr(C)]
pub(crate) struct ViewState {
    pub factory: ID2D1Factory8,
    pub render_target: Option<ID2D1HwndRenderTarget>,
}

impl ViewState {
    fn new() -> Result<Self, Error> {
        let factory = unsafe {
            // SAFETY: all inputs are valid
            D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)?
        };

        Ok(ViewState { factory, render_target: None })
    }

    /// Initalizes all the state
    ///
    /// SAFETY: `hwnd` must be a valid handle
    #[allow(unsafe_op_in_unsafe_fn)]
    pub unsafe fn init(&mut self, hwnd: HWND) -> Result<(), Error> {
        debug_assert!(!hwnd.is_invalid(), "`hwnd` at this point should be a valid window handle");
        self.init_graphics(hwnd)?;

        Ok(())
    }

    /// Releases all the state
    pub fn release(&mut self) {
        self.release_graphics();
    }

    /// Initializes the graphics API
    ///
    /// SAFETY: `hwnd` must be a valid handle
    pub unsafe fn init_graphics(&mut self, hwnd: HWND) -> Result<(), Error> {
        if self.render_target.is_none() {
            debug_assert!(!hwnd.is_invalid(), "`hwnd` at this point should be a valid window handle");

            let rect = unsafe {
                let mut rect = RECT::default();
                // SAFETY:
                //  - hwnd is a valid handle
                //  - &raw mut rect is pointing to a valid memory addres
                GetClientRect(hwnd, &raw mut rect)?;
                rect
            };

            let render_target_properties = Default::default();
            let hwnd_render_target_properties = windows::Win32::Graphics::Direct2D::D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: size_of_rect(rect),
                presentOptions: Default::default(),
            };

            let render_target = unsafe {
                // SAFETY: All these poitners point to valid values
                self.factory
                    .CreateHwndRenderTarget(&raw const render_target_properties, &raw const hwnd_render_target_properties)?
            };

            let _ = self.render_target.insert(render_target);
        }

        Ok(())
    }

    /// Releases the graphics API
    pub fn release_graphics(&mut self) {
        self.render_target = None;
    }
}

impl Drop for ViewState {
    fn drop(&mut self) {
        self.release()
    }
}

fn size_of_rect(rect: RECT) -> D2D_SIZE_U {
    D2D_SIZE_U {
        width: (rect.right - rect.left) as u32,
        height: i32::abs(rect.top - rect.bottom) as u32,
    }
}
