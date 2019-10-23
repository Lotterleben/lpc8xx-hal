//! Higher-level sleep API
//!
//! This module provides a higher-level API layer that can be used to put the
//! microcontroller to sleep for a given amount of time.
//!
//! Both sleeping via busy waiting and via regular sleep mode are supported.
//! Please refer to [`sleep::Busy`] and [`sleep::Regular`] for more details.
//!
//! [`sleep::Busy`]: struct.Busy.html
//! [`sleep::Regular`]: struct.Regular.html

use cortex_m::{asm, interrupt};
use embedded_hal::prelude::*;
use nb;

use crate::{
    clock::{self, Ticks},
    pac::{self, Interrupt, NVIC},
    pmu,
    wkt::{self, WKT},
};

/// Trait for putting the processor to sleep
///
/// There will typically one implementation of `Sleep` per sleep mode that is
/// available on a given microcontroller.
pub trait Sleep<Clock>
where
    Clock: clock::Enabled,
{
    /// Puts the processor to sleep for the given number of ticks of the clock
    fn sleep<'clock, T>(&mut self, ticks: T)
    where
        Clock: 'clock,
        T: Into<Ticks<'clock, Clock>>;
}

/// Sleep mode based on busy waiting
///
/// Provides a [`Sleep`] implementation based on busy waiting and uses the [WKT]
/// to measure the time. An interrupt handler is not required.
///
/// Only clocks that the WKT supports can be used. See [`wkt::Clock`] for more
/// details.
///
/// Since this sleep mode waits busily, which is very energy-inefficient, it is
/// only suitable for simple examples, or very short wait times.
///
/// # Examples
///
/// ``` no_run
/// use lpc82x_hal::prelude::*;
/// use lpc82x_hal::{
///     sleep,
///     Peripherals,
/// };
/// use lpc82x_hal::clock::Ticks;
///
/// let mut p = Peripherals::take().unwrap();
///
/// let mut syscon = p.SYSCON.split();
/// let mut wkt    = p.WKT.enable(&mut syscon.handle);
///
/// let clock = syscon.irc_derived_clock;
///
/// let mut sleep = sleep::Busy::prepare(&mut wkt);
///
/// let delay = Ticks { value: 750_000, clock: &clock }; // 1000 ms
/// sleep.sleep(delay);
/// ```
pub struct Busy<'wkt> {
    wkt: &'wkt mut WKT,
}

impl<'wkt> Busy<'wkt> {
    /// Prepare busy sleep mode
    ///
    /// Returns an instance of `sleep::Busy`, which implements [`Sleep`] and can
    /// therefore be used to put the microcontroller to sleep.
    ///
    /// Requires a mutable reference to [`WKT`]. The reference will be borrowed
    /// for as long as the `sleep::Busy` instance exists, as it will be needed
    /// to count down the time in every call to [`Sleep::sleep`].
    pub fn prepare(wkt: &'wkt mut WKT) -> Self {
        Busy { wkt: wkt }
    }
}

impl<'wkt, Clock> Sleep<Clock> for Busy<'wkt>
where
    Clock: clock::Enabled + wkt::Clock,
{
    fn sleep<'clock, T>(&mut self, ticks: T)
    where
        Clock: 'clock,
        T: Into<Ticks<'clock, Clock>>,
    {
        let ticks: Ticks<Clock> = ticks.into();

        // If we try to sleep for zero cycles, we'll never wake up again.
        if ticks.value == 0 {
            return;
        }

        self.wkt.start(ticks.value);
        while let Err(nb::Error::WouldBlock) = self.wkt.wait() {
            asm::nop();
        }
    }
}

/// Regular sleep mode
///
/// Provides a [`Sleep`] implementation for the regular sleep mode and uses the
/// [WKT] to wake the microcontroller up again, at the right time. Only clocks
/// that the WKT supports can be used. See [`wkt::Clock`] for more details.
///
/// # Examples
///
/// ``` no_run
/// use lpc82x_hal::prelude::*;
/// use lpc82x_hal::{
///     raw,
///     sleep,
///     Peripherals,
/// };
/// use lpc82x_hal::clock::Ticks;
///
/// let mut p = Peripherals::take().unwrap();
///
/// let mut pmu    = p.PMU.split();
/// let mut syscon = p.SYSCON.split();
/// let mut wkt    = p.WKT.enable(&mut syscon.handle);
///
/// let clock = syscon.irc_derived_clock;
///
/// let mut sleep = sleep::Regular::prepare(
///     &mut p.NVIC,
///     &mut pmu.handle,
///     &mut p.SCB,
///     &mut wkt,
/// );
///
/// let delay = Ticks { value: 750_000, clock: &clock }; // 1000 ms
///
/// // This will put the microcontroller into sleep mode.
/// sleep.sleep(delay);
/// ```
pub struct Regular<'r> {
    pmu: &'r mut pmu::Handle,
    scb: &'r mut pac::SCB,
    wkt: &'r mut WKT,
}

impl<'r> Regular<'r> {
    /// Prepare regular sleep mode
    ///
    /// Returns an instance of `sleep::Regular`, which implements [`Sleep`] and
    /// can therefore be used to put the microcontroller to sleep.
    ///
    /// Requires references to various peripherals, which will be borrowed for
    /// as long as the `sleep::Regular` instance exists, as they will be needed
    /// for every call to [`Sleep::sleep`].
    pub fn prepare(pmu: &'r mut pmu::Handle, scb: &'r mut pac::SCB, wkt: &'r mut WKT) -> Self {
        Regular {
            pmu: pmu,
            scb: scb,
            wkt: wkt,
        }
    }
}

impl<'r, Clock> Sleep<Clock> for Regular<'r>
where
    Clock: clock::Enabled + wkt::Clock,
{
    fn sleep<'clock, T>(&mut self, ticks: T)
    where
        Clock: 'clock,
        T: Into<Ticks<'clock, Clock>>,
    {
        let ticks: Ticks<Clock> = ticks.into();

        // If we try to sleep for zero cycles, we'll never wake up again.
        if ticks.value == 0 {
            return;
        }

        self.wkt.select_clock::<Clock>();
        self.wkt.start(ticks.value);

        // Within the this closure, interrupts are enabled, but interrupt
        // handlers won't run. This means that we'll exit sleep mode when the
        // WKT interrupt is fired, but there won't be an interrupt handler that
        // will require the WKT's alarm flag to be reset. This means the `wait`
        // method can use the alarm flag, which would otherwise need to be reset
        // to exit the interrupt handler.
        interrupt::free(|_| {
            // Safe, because this is not going to interfere with the critical
            // section.
            unsafe { NVIC::unmask(Interrupt::WKT) };

            while let Err(nb::Error::WouldBlock) = self.wkt.wait() {
                self.pmu.enter_sleep_mode(self.scb);
            }

            // If we don't do this, the (possibly non-existing) interrupt
            // handler will be called as soon as we exit this closure.
            NVIC::mask(Interrupt::WKT);
        });
    }
}
