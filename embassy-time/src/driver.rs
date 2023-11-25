//! Time driver interface
//!
//! This module defines the interface a driver needs to implement to power the `embassy_time` module.
//!
//! # Implementing a driver
//!
//! - Define a struct `MyDriver`
//! - Implement [`Driver`] for it
//! - Register it as the global driver with [`time_driver_impl`](crate::time_driver_impl).
//! - Enable the Cargo features `embassy-executor/time` and one of `embassy-time/tick-*` corresponding to the
//!   tick rate of your driver.
//!
//! If you wish to make the tick rate configurable by the end user, you should do so by exposing your own
//! Cargo features and having each enable the corresponding `embassy-time/tick-*`.
//!
//! # Linkage details
//!
//! Instead of the usual "trait + generic params" approach, calls from embassy to the driver are done via `extern` functions.
//!
//! `embassy` internally defines the driver functions as `extern "Rust" { fn _embassy_time_now() -> u64; }` and calls them.
//! The driver crate defines the functions as `#[no_mangle] fn _embassy_time_now() -> u64`. The linker will resolve the
//! calls from the `embassy` crate to call into the driver crate.
//!
//! If there is none or multiple drivers in the crate tree, linking will fail.
//!
//! This method has a few key advantages for something as foundational as timekeeping:
//!
//! - The time driver is available everywhere easily, without having to thread the implementation
//!   through generic parameters. This is especially helpful for libraries.
//! - It means comparing `Instant`s will always make sense: if there were multiple drivers
//!   active, one could compare an `Instant` from driver A to an `Instant` from driver B, which
//!   would yield incorrect results.
//!
//! # Example
//!
//! ```
//! use embassy_time::driver::{Driver, AlarmHandle};
//!
//! struct MyDriver{} // not public!
//! embassy_time::time_driver_impl!(static DRIVER: MyDriver = MyDriver{});
//!
//! impl Driver for MyDriver {
//!     fn now(&self) -> u64 {
//!         todo!()
//!     }
//!     unsafe fn allocate_alarm(&self) -> Option<AlarmHandle> {
//!         todo!()
//!     }
//!     fn set_alarm_callback(&self, alarm: AlarmHandle, callback: fn(*mut ()), ctx: *mut ()) {
//!         todo!()
//!     }
//!     fn set_alarm(&self, alarm: AlarmHandle, timestamp: u64) -> bool {
//!         todo!()
//!     }
//! }
//! ```

/// Alarm handle, assigned by the driver.
#[derive(Clone, Copy)]
pub struct AlarmHandle {
    id: u8,
}

impl AlarmHandle {
    /// 生成AlarmHandle
    ///
    /// 安全提示：只能由当前全局driver impl调用。所有的“AlarmHandle”实例都是
    /// 在不安全的代码中自行创建的，比如索引操作。
    /// ---
    ///
    /// Create an AlarmHandle
    ///
    /// Safety: May only be called by the current global Driver impl.
    /// The impl is allowed to rely on the fact that all `AlarmHandle` instances
    /// are created by itself in unsafe code (e.g. indexing operations)
    pub unsafe fn new(id: u8) -> Self {
        Self { id }
    }

    /// 获取当前AlarmHandle实例的id
    /// ---
    /// Get the ID of the AlarmHandle.
    pub fn id(&self) -> u8 {
        self.id
    }
}

/// Time driver
pub trait Driver: Send + Sync + 'static {
    /// 以ticks（u64类型）的方式返回当前的时间戳
    ///
    /// 该函数的实现必须保证：
    /// - 函数必须是单调增的，调用now()的返回值必须大于或
    /// 等于前一次调用的返回值。时间不能“回到过去”。
    /// - 不会溢出。在足够长的时间内不会溢出。比方说一万年
    /// (一万年后人类社会应该都毁灭了)。这意味着如果硬件只
    /// 有16/32位，必须扩展到64位，比如在软件层面处理溢出
    /// 或是把多个计时器串联起来。
    /// ---
    /// Return the current timestamp in ticks.
    ///
    /// Implementations MUST ensure that:
    /// - This is guaranteed to be monotonic, i.e. a call to now() will always return
    ///   a greater or equal value than earler calls. Time can't "roll backwards".
    /// - It "never" overflows. It must not overflow in a sufficiently long time frame, say
    ///   in 10_000 years (Human civilization is likely to already have self-destructed
    ///   10_000 years from now.). This means if your hardware only has 16bit/32bit timers
    ///   you MUST extend them to 64-bit, for example by counting overflows in software,
    ///   or chaining multiple timers together.
    fn now(&self) -> u64;

    /// 尝试分配一个报警执行(AlarmHandle)，如果没有报警或分配失败则返回None。
    /// 初始化时报警执行回调为空，返回空的`ctx`指针。
    ///
    /// # 安全提示
    /// 在设置报警执行回调之前生成报警会导致未定义行为！
    /// ---
    /// Try allocating an alarm handle. Returns None if no alarms left.
    /// Initially the alarm has no callback set, and a null `ctx` pointer.
    ///
    /// # Safety
    /// It is UB to make the alarm fire before setting a callback.
    unsafe fn allocate_alarm(&self) -> Option<AlarmHandle>;

    /// 设置当报警触发时执行的回调函数。
    /// 该回调函数可以被任意上下文调用(中断或线程模式)
    /// Sets the callback function to be called when the alarm triggers.
    /// The callback may be called from any context (interrupt or thread mode).
    fn set_alarm_callback(&self, alarm: AlarmHandle, callback: fn(*mut ()), ctx: *mut ());

    /// 在给定的时间戳处设置警报。当前时间达到该时间戳时，提供的回调函数将被调用。
    ///
    /// 驱动实现应确保回调函数不会被`set_alarm`同步调用。如果时间戳的时间点已经过去，
    /// 应该返回`false`并不设置警报，否则应该返回`true`并安排触发时调用，而不是同步调用。
    ///
    /// 如果回调被调用，now()确定会返回大于或等于给定的时间戳。
    ///
    /// 同一时刻每个`AlarmHandle`仅有一个警报会触发，以前如果有设置报警，将会被覆盖。
    /// ---
    /// Sets an alarm at the given timestamp. When the current timestamp reaches the alarm
    /// timestamp, the provided callback function will be called.
    ///
    /// The `Driver` implementation should guarantee that the alarm callback is never called synchronously from `set_alarm`.
    /// Rather - if `timestamp` is already in the past - `false` should be returned and alarm should not be set,
    /// or alternatively, the driver should return `true` and arrange to call the alarm callback as soon as possible, but not synchronously.
    ///
    /// When callback is called, it is guaranteed that now() will return a value greater or equal than timestamp.
    ///
    /// Only one alarm can be active at a time for each AlarmHandle. This overwrites any previously-set alarm if any.
    fn set_alarm(&self, alarm: AlarmHandle, timestamp: u64) -> bool;
}

extern "Rust" {
    fn _embassy_time_now() -> u64;
    fn _embassy_time_allocate_alarm() -> Option<AlarmHandle>;
    fn _embassy_time_set_alarm_callback(alarm: AlarmHandle, callback: fn(*mut ()), ctx: *mut ());
    fn _embassy_time_set_alarm(alarm: AlarmHandle, timestamp: u64) -> bool;
}

/// See [`Driver::now`]
pub fn now() -> u64 {
    unsafe { _embassy_time_now() }
}

/// See [`Driver::allocate_alarm`]
///
/// Safety: it is UB to make the alarm fire before setting a callback.
pub unsafe fn allocate_alarm() -> Option<AlarmHandle> {
    _embassy_time_allocate_alarm()
}

/// See [`Driver::set_alarm_callback`]
pub fn set_alarm_callback(alarm: AlarmHandle, callback: fn(*mut ()), ctx: *mut ()) {
    unsafe { _embassy_time_set_alarm_callback(alarm, callback, ctx) }
}

/// See [`Driver::set_alarm`]
pub fn set_alarm(alarm: AlarmHandle, timestamp: u64) -> bool {
    unsafe { _embassy_time_set_alarm(alarm, timestamp) }
}

/// Set the time Driver implementation.
///
/// See the module documentation for an example.
#[macro_export]
macro_rules! time_driver_impl {
    (static $name:ident: $t: ty = $val:expr) => {
        static $name: $t = $val;

        #[no_mangle]
        fn _embassy_time_now() -> u64 {
            <$t as $crate::driver::Driver>::now(&$name)
        }

        #[no_mangle]
        unsafe fn _embassy_time_allocate_alarm() -> Option<$crate::driver::AlarmHandle> {
            <$t as $crate::driver::Driver>::allocate_alarm(&$name)
        }

        #[no_mangle]
        fn _embassy_time_set_alarm_callback(alarm: $crate::driver::AlarmHandle, callback: fn(*mut ()), ctx: *mut ()) {
            <$t as $crate::driver::Driver>::set_alarm_callback(&$name, alarm, callback, ctx)
        }

        #[no_mangle]
        fn _embassy_time_set_alarm(alarm: $crate::driver::AlarmHandle, timestamp: u64) -> bool {
            <$t as $crate::driver::Driver>::set_alarm(&$name, alarm, timestamp)
        }
    };
}
