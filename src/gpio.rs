//! API for General Purpose I/O (GPIO)
//!
//! The entry point to this API is [`GPIO`]. It can be used to initialize the
//! peripheral, and is required to convert instances of [`Pin`] to a
//! [`GpioPin`], which provides the core GPIO API.
//!
//! The GPIO peripheral is described in the following user manuals:
//! - LPC82x user manual, chapter 9
//! - LPC84x user manual, chapter 12
//!
//! # Examples
//!
//! Initialize a GPIO pin and set its output to HIGH:
//!
//! ``` no_run
//! use lpc8xx_hal::{
//!     prelude::*,
//!     Peripherals,
//!     gpio,
//! };
//!
//! let mut p = Peripherals::take().unwrap();
//!
//! let mut syscon = p.SYSCON.split();
//!
//! #[cfg(feature = "82x")]
//! let gpio = p.GPIO;
//! #[cfg(feature = "845")]
//! let gpio = p.GPIO.enable(&mut syscon.handle);
//!
//! let pio0_12 = p.pins.pio0_12.into_output_pin(
//!     gpio.tokens.pio0_12,
//!     gpio::Level::High,
//! );
//! ```
//!
//! Please refer to the [examples in the repository] for more example code.
//!
//! [`GPIO`]: struct.GPIO.html
//! [`Pin`]: ../pins/struct.Pin.html
//! [`GpioPin`]: struct.GpioPin.html
//! [examples in the repository]: https://github.com/lpc-rs/lpc8xx-hal/tree/master/examples

use core::marker::PhantomData;

use embedded_hal::digital::v2::{
    InputPin, OutputPin, StatefulOutputPin, ToggleableOutputPin,
};
use embedded_hal_alpha::digital::{
    InputPin as InputPinAlpha, OutputPin as OutputPinAlpha,
    StatefulOutputPin as StatefulOutputPinAlpha,
    ToggleableOutputPin as ToggleableOutputPinAlpha,
};
use void::Void;

use crate::{
    init_state, pac,
    pins::{self, Token},
    syscon,
};

#[cfg(feature = "845")]
use crate::pac::gpio::{CLR, DIRCLR, DIRSET, NOT, PIN, SET};
#[cfg(feature = "82x")]
use crate::pac::gpio::{
    CLR0 as CLR, DIRCLR0 as DIRCLR, DIRSET0 as DIRSET, NOT0 as NOT,
    PIN0 as PIN, SET0 as SET,
};

use self::direction::{Direction, DynamicPinErr};

/// Interface to the GPIO peripheral
///
/// Controls the GPIO peripheral. Can be used to enable, disable, or free the
/// peripheral. For GPIO-functionality directly related to pins, please refer
/// to [`GpioPin`].
///
/// Use [`Peripherals`] to gain access to an instance of this struct.
///
/// Please refer to the [module documentation] for more information.
///
/// [`GpioPin`]: struct.GpioPin.html
/// [`Peripherals`]: ../struct.Peripherals.html
/// [module documentation]: index.html
pub struct GPIO<State = init_state::Enabled> {
    pub(crate) gpio: pac::GPIO,
    _state: PhantomData<State>,

    /// Tokens representing all pins
    ///
    /// Since the [`enable`] and [`disable`] methods consume `self`, they can
    /// only be called, if all tokens are available. This means, any tokens that
    /// have been moved out while the peripheral was enabled, prevent the
    /// peripheral from being disabled (unless those tokens are moved back into
    /// their original place).
    ///
    /// As using a pin for GPIO requires such a token, it is impossible to
    /// disable the GPIO peripheral while pins are used for GPIO.
    ///
    /// [`enable`]: #method.enable
    /// [`disable`]: #method.disable
    pub tokens: pins::Tokens<State>,
}

impl<State> GPIO<State> {
    pub(crate) fn new(gpio: pac::GPIO) -> Self {
        GPIO {
            gpio,
            _state: PhantomData,

            tokens: pins::Tokens::new(),
        }
    }

    /// Return the raw peripheral
    ///
    /// This method serves as an escape hatch from the HAL API. It returns the
    /// raw peripheral, allowing you to do whatever you want with it, without
    /// limitations imposed by the API.
    ///
    /// If you are using this method because a feature you need is missing from
    /// the HAL API, please [open an issue] or, if an issue for your feature
    /// request already exists, comment on the existing issue, so we can
    /// prioritize it accordingly.
    ///
    /// [open an issue]: https://github.com/lpc-rs/lpc8xx-hal/issues
    pub fn free(self) -> pac::GPIO {
        self.gpio
    }
}

impl GPIO<init_state::Disabled> {
    /// Enable the GPIO peripheral
    ///
    /// This method is only available, if `GPIO` is in the [`Disabled`] state.
    /// Code that attempts to call this method when the peripheral is already
    /// enabled will not compile.
    ///
    /// Consumes this instance of `GPIO` and returns another instance that has
    /// its `State` type parameter set to [`Enabled`].
    ///
    /// [`Disabled`]: ../init_state/struct.Disabled.html
    /// [`Enabled`]: ../init_state/struct.Enabled.html
    pub fn enable(
        self,
        syscon: &mut syscon::Handle,
    ) -> GPIO<init_state::Enabled> {
        syscon.enable_clock(&self.gpio);

        // Only works, if all tokens are available.
        let tokens = self.tokens.switch_state();

        GPIO {
            gpio: self.gpio,
            _state: PhantomData,
            tokens,
        }
    }
}

impl GPIO<init_state::Enabled> {
    /// Disable the GPIO peripheral
    ///
    /// This method is only available, if `GPIO` is in the [`Enabled`] state.
    /// Code that attempts to call this method when the peripheral is already
    /// disabled will not compile.
    ///
    /// Consumes this instance of `GPIO` and returns another instance that has
    /// its `State` type parameter set to [`Disabled`].
    ///
    /// [`Enabled`]: ../init_state/struct.Enabled.html
    /// [`Disabled`]: ../init_state/struct.Disabled.html
    pub fn disable(
        self,
        syscon: &mut syscon::Handle,
    ) -> GPIO<init_state::Disabled> {
        syscon.disable_clock(&self.gpio);

        // Only works, if all tokens are available.
        let tokens = self.tokens.switch_state();

        GPIO {
            gpio: self.gpio,
            _state: PhantomData,
            tokens,
        }
    }
}

/// A pin used for general purpose I/O (GPIO)
///
/// You can get access to an instance of this struct by switching a pin to the
/// GPIO state, using [`Pin::into_input_pin`] or [`Pin::into_output_pin`].
///
/// # `embedded-hal` traits
/// - While in input mode
///   - [`embedded_hal::digital::v2::InputPin`] for reading the pin state
/// - While in output mode
///   - [`embedded_hal::digital::v2::OutputPin`] for setting the pin state
///   - [`embedded_hal::digital::v2::StatefulOutputPin`] for reading the pin output state
///   - [`embedded_hal::digital::v2::ToggleableOutputPin`] for toggling the pin state
///
/// [`Pin::into_input_pin`]: ../pins/struct.Pin.html#method.into_input_pin
/// [`Pin::into_output_pin`]: ../pins/struct.Pin.html#method.into_output_pin
/// [`embedded_hal::digital::v2::InputPin`]: #impl-InputPin
/// [`embedded_hal::digital::v2::OutputPin`]: #impl-OutputPin
/// [`embedded_hal::digital::v2::StatefulOutputPin`]: #impl-StatefulOutputPin
/// [`embedded_hal::digital::v2::ToggleableOutputPin`]: #impl-ToggleableOutputPin
pub struct GpioPin<T, D> {
    token: pins::Token<T, init_state::Enabled>,
    _direction: D,
}

/// TODO docs
pub struct DynamicGpioPin<D> {
    // we can't use tokens to ID the pin so let's YOLO for now and store the same info but different
    _port: usize, // TODO: why is this a usize in the original Trait? seems wasteful?
    _mask: u32,
    _direction: D,
}

// TODO only impl for D = Dynamic? Do we even need a <D> then?
impl<D> DynamicGpioPin<D>
where
    D: Direction,
{
    /// TODO docs
    pub(crate) fn new<T: pins::Trait>(
        _token: Token<T, init_state::Enabled>,
        arg: D::SwitchArg,
    ) -> Self {
        // This is sound, as we only write to stateless registers, restricting
        // ourselves to the bit that belongs to the pin represented by `T`.
        // Since all other instances of `GpioPin` and `DynamicGpioPin` are doing the same, there are
        // no race conditions.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);
        let direction = D::switch::<T>(&registers, arg);

        Self {
            _port: T::PORT,
            _mask: T::MASK,
            _direction: direction,
        }
    }
}

impl DynamicGpioPin<direction::Dynamic> {
    /// TODO add docs
    pub fn direction_is_output(&self) -> bool {
        return self._direction.is_output;
    }

    /// TODO add docs
    pub fn direction_is_input(&self) -> bool {
        return !self.direction_is_output();
    }

    /// TODO docs
    pub fn is_high(&self) -> bool {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        // TODO no copypasta
        // is_high::<T>(&registers)
        registers.pin[self._port].read().port().bits() & self._mask
            == self._mask
    }

    /// Switch pin direction to input. If the pin is already an input pin, this does nothing.
    pub fn switch_to_input(&mut self) {
        // TODO decide what I want here: rm Dynamic direction or not?
        if self.direction_is_output() == false {
            return;
        }

        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        // switch direction
        // TODO no copypasta
        //set_direction_input::<T>(&registers);
        registers.dirclr[self._port]
            .write(|w| unsafe { w.dirclrp().bits(self._mask) });
        self._direction.is_output = false;
    }

    /// Switch pin direction to output with output level set to `level`.
    /// If the pin is already an output pin, this function only switches its level to `level`.
    pub fn switch_to_output(&mut self, level: Level) {
        // we are already in output, nothing else to do
        if self.direction_is_output() {
            return;
        }

        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        // First set the output level, before we switch the mode.
        match level {
            Level::High => {
                // TODO no copypasta
                // self.set_high()
                registers.set[self._port]
                    .write(|w| unsafe { w.setp().bits(self._mask) });
            }
            Level::Low => {
                // TODO no copypasta
                // self.set_low()},
                registers.clr[self._port]
                    .write(|w| unsafe { w.clrp().bits(self._mask) });
            }
        }

        // Now that the output level is configured, we can safely switch to
        // output mode, without risking an undesired signal between now and
        // the first call to `set_high`/`set_low`.
        // TODO no copypasta
        //set_direction_output::<T>(&registers);
        registers.dirset[self._port]
            .write(|w| unsafe { w.dirsetp().bits(self._mask) });
        self._direction.is_output = true;
    }
}

impl<T, D> GpioPin<T, D>
where
    T: pins::Trait,
    D: Direction,
{
    pub(crate) fn new(
        token: Token<T, init_state::Enabled>,
        arg: D::SwitchArg,
    ) -> Self {
        // This is sound, as we only write to stateless registers, restricting
        // ourselves to the bit that belongs to the pin represented by `T`.
        // Since all other instances of `GpioPin` are doing the same, there are
        // no race conditions.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);
        let direction = D::switch::<T>(&registers, arg);

        Self {
            token,
            _direction: direction,
        }
    }
}

impl<T> GpioPin<T, direction::Input>
where
    T: pins::Trait,
{
    /// Set pin direction to output
    ///
    /// This method is only available while the pin is in input mode.
    ///
    /// Consumes the pin instance and returns a new instance that is in output
    /// mode, making the methods to set the output level available.
    pub fn into_output(self, initial: Level) -> GpioPin<T, direction::Output> {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        let direction = direction::Output::switch::<T>(&registers, initial);

        GpioPin {
            token: self.token,
            _direction: direction,
        }
    }

    /// Set pin direction to dynamic (i.e. changeable at runtime)
    ///
    /// This method is only available when the pin is not already in dynamic mode.
    ///
    /// Consumes the pin instance and returns a new instance that is in dynamic
    /// mode, making the methods to change direction as well as read/set levels
    /// (depending on the current diection) available.
    pub fn into_dynamic(
        self,
        initial_level: Level,
        initial_direction: pins::DynamicPinDirection,
    ) -> GpioPin<T, direction::Dynamic> {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        GpioPin {
            token: self.token,
            // always switch to ensure initial level and direction are set correctly
            _direction: direction::Dynamic::switch::<T>(
                &registers,
                (initial_level, initial_direction),
            ),
        }
    }

    /// Indicates wether the pin input is HIGH
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to input.
    ///
    /// See [`Pin::into_input_pin`] and [`into_input`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_input_pin`]: ../pins/struct.Pin.html#method.into_input_pin
    /// [`into_input`]: #method.into_input
    pub fn is_high(&self) -> bool {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        is_high::<T>(&registers)
    }

    /// Indicates wether the pin input is LOW
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to input.
    ///
    /// See [`Pin::into_input_pin`] and [`into_input`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_input_pin`]: ../pins/struct.Pin.html#method.into_input_pin
    /// [`into_input`]: #method.into_input
    pub fn is_low(&self) -> bool {
        !self.is_high()
    }
}

impl<T> GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    /// Set pin direction to input
    ///
    /// This method is only available while the pin is in output mode.
    ///
    /// Consumes the pin instance and returns a new instance that is in output
    /// mode, making the methods to set the output level available.
    pub fn into_input(self) -> GpioPin<T, direction::Input> {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        let direction = direction::Input::switch::<T>(&registers, ());

        GpioPin {
            token: self.token,
            _direction: direction,
        }
    }

    /// Set pin direction to dynamic (i.e. changeable at runtime)
    ///
    /// This method is only available when the pin is not already in dynamic mode.
    ///
    /// Consumes the pin instance and returns a new instance that is in dynamic
    /// mode, making the methods to change direction as well as read/set levels
    /// (depending on the current diection) available.
    pub fn into_dynamic(
        self,
        initial_level: Level,
        initial_direction: pins::DynamicPinDirection,
    ) -> GpioPin<T, direction::Dynamic> {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        GpioPin {
            token: self.token,
            // always switch to ensure initial level and direction are set correctly
            _direction: direction::Dynamic::switch::<T>(
                &registers,
                (initial_level, initial_direction),
            ),
        }
    }

    /// Set the pin output to HIGH
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to output.
    ///
    /// See [`Pin::into_output_pin`] and [`into_output`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_output_pin`]: ../pins/struct.Pin.html#method.into_output_pin
    /// [`into_output`]: #method.into_output
    pub fn set_high(&mut self) {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        set_high::<T>(&registers);
    }

    /// Set the pin output to LOW
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to output.
    ///
    /// See [`Pin::into_output_pin`] and [`into_output`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_output_pin`]: ../pins/struct.Pin.html#method.into_output_pin
    /// [`into_output`]: #method.into_output
    pub fn set_low(&mut self) {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        set_low::<T>(&registers);
    }

    /// Indicates whether the pin output is currently set to HIGH
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to output.
    ///
    /// See [`Pin::into_output_pin`] and [`into_output`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_output_pin`]: ../pins/struct.Pin.html#method.into_output_pin
    /// [`into_output`]: #method.into_output
    pub fn is_set_high(&self) -> bool {
        // This is sound, as we only read a bit from a register.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        is_high::<T>(&registers)
    }

    /// Indicates whether the pin output is currently set to LOW
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to output.
    ///
    /// See [`Pin::into_output_pin`] and [`into_output`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_output_pin`]: ../pins/struct.Pin.html#method.into_output_pin
    /// [`into_output`]: #method.into_output
    pub fn is_set_low(&self) -> bool {
        !self.is_set_high()
    }

    /// Toggle the pin output
    ///
    /// This method is only available, if two conditions are met:
    /// - The pin is in the GPIO state.
    /// - The pin direction is set to output.
    ///
    /// See [`Pin::into_output_pin`] and [`into_output`]. Unless both of these
    /// conditions are met, code trying to call this method will not compile.
    ///
    /// [`Pin::into_output_pin`]: ../pins/struct.Pin.html#method.into_output_pin
    /// [`into_output`]: #method.into_output
    pub fn toggle(&mut self) {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        registers.not[T::PORT].write(|w| unsafe { w.notp().bits(T::MASK) });
    }
}

impl<T> GpioPin<T, direction::Dynamic>
where
    T: pins::Trait,
{
    /// Tell us whether this pin's direction is currently set to Output.
    pub fn direction_is_output(&self) -> bool {
        return self._direction.current_direction
            == pins::DynamicPinDirection::Output;
    }

    /// Tell us whether this pin's direction is currently set to Input.
    pub fn direction_is_input(&self) -> bool {
        return !self.direction_is_output();
    }

    /// Switch pin direction to input. If the pin is already an input pin, this does nothing.
    pub fn switch_to_input(&mut self) {
        if self._direction.current_direction == pins::DynamicPinDirection::Input
        {
            return;
        }

        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        // switch direction
        set_direction_input::<T>(&registers);
        self._direction.current_direction = pins::DynamicPinDirection::Input;
    }

    /// Switch pin direction to output with output level set to `level`.
    /// If the pin is already an output pin, this function only switches its level to `level`.
    pub fn switch_to_output(&mut self, level: Level) {
        // First set the output level, before we switch the mode.
        match level {
            Level::High => self.set_high(),
            Level::Low => self.set_low(),
        }

        // we are already in output, nothing else to do
        if self._direction.current_direction
            == pins::DynamicPinDirection::Output
        {
            return;
        }

        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        // Now that the output level is configured, we can safely switch to
        // output mode, without risking an undesired signal between now and
        // the first call to `set_high`/`set_low`.
        set_direction_output::<T>(&registers);
        self._direction.current_direction = pins::DynamicPinDirection::Output;
    }

    /// Set the pin level to High.
    /// Note that this will be executed regardless of the current pin direction.
    /// This enables you to set the initial pin level *before* switching to output
    pub fn set_high(&mut self) {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        set_high::<T>(&registers);
    }

    /// Set the pin level to Low.
    /// Note that this will be executed regardless of the current pin direction.
    /// This enables you to set the initial pin level *before* switching to output
    pub fn set_low(&mut self) {
        // This is sound, as we only do a stateless write to a bit that no other
        // `GpioPin` instance writes to.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        set_low::<T>(&registers);
    }

    /// Indicates whether the voltage at this pin is currently set to HIGH
    /// This can be used when the pin is in any direction:
    ///
    /// If it is currently an Output pin, it indicates whether the pin output is set to HIGH
    /// If it is currently an Input pin, it indicates wether the pin input is HIGH
    ///
    /// This method is only available, if the pin has been set to dynamic mode.
    /// See [`Pin::into_dynamic_pin`].
    /// Unless this condition is met, code trying to call this method will not compile.
    pub fn is_high(&self) -> bool {
        // This is sound, as we only read a bit from a register.
        let gpio = unsafe { &*pac::GPIO::ptr() };
        let registers = Registers::new(gpio);

        is_high::<T>(&registers)
    }

    /// Indicates whether the voltage at this pin is currently set to LOW
    /// This can be used when the pin is in any direction:
    ///
    /// If it is currently an Output pin, it indicates whether the pin output is set to LOW
    /// If it is currently an Input pin, it indicates wether the pin input is LOW
    ///
    /// This method is only available, if the pin has been set to dynamic mode.
    /// See [`Pin::into_dynamic_pin`].
    /// Unless this condition is met, code trying to call this method will not compile.
    pub fn is_low(&self) -> bool {
        !self.is_high()
    }
}

impl<T> OutputPin for GpioPin<T, direction::Dynamic>
where
    T: pins::Trait,
{
    type Error = DynamicPinErr;

    fn set_high(&mut self) -> Result<(), Self::Error> {
        // NOTE: this check is kind of redundant but since both `set_high()`s are public I
        // didn't want to either leave it out of `self.set_high()` or return an OK here
        // when there's really an error
        // (applies to all Dynamic Pin impls)
        match self._direction.current_direction {
            pins::DynamicPinDirection::Output => {
                // Call the inherent method defined above.
                Ok(self.set_high())
            }
            pins::DynamicPinDirection::Input => {
                Err(Self::Error::WrongDirection)
            }
        }
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        match self._direction.current_direction {
            pins::DynamicPinDirection::Output => {
                // Call the inherent method defined above.
                Ok(self.set_low())
            }
            pins::DynamicPinDirection::Input => {
                Err(Self::Error::WrongDirection)
            }
        }
    }
}

impl<T> StatefulOutputPin for GpioPin<T, direction::Dynamic>
where
    T: pins::Trait,
{
    fn is_set_high(&self) -> Result<bool, Self::Error> {
        match self._direction.current_direction {
            pins::DynamicPinDirection::Output => {
                // Re-use level reading function
                Ok(self.is_high())
            }
            pins::DynamicPinDirection::Input => {
                Err(Self::Error::WrongDirection)
            }
        }
    }

    fn is_set_low(&self) -> Result<bool, Self::Error> {
        match self._direction.current_direction {
            pins::DynamicPinDirection::Output => {
                // Re-use level reading function
                Ok(self.is_low())
            }
            pins::DynamicPinDirection::Input => {
                Err(Self::Error::WrongDirection)
            }
        }
    }
}

impl<T> InputPin for GpioPin<T, direction::Dynamic>
where
    T: pins::Trait,
{
    type Error = DynamicPinErr;

    fn is_high(&self) -> Result<bool, Self::Error> {
        match self._direction.current_direction {
            pins::DynamicPinDirection::Output => {
                Err(Self::Error::WrongDirection)
            }
            pins::DynamicPinDirection::Input => {
                // Call the inherent method defined above.
                Ok(self.is_high())
            }
        }
    }

    fn is_low(&self) -> Result<bool, Self::Error> {
        match self._direction.current_direction {
            pins::DynamicPinDirection::Output => {
                Err(Self::Error::WrongDirection)
            }
            pins::DynamicPinDirection::Input => {
                // Call the inherent method defined above.
                Ok(self.is_low())
            }
        }
    }
}

impl<T> InputPin for GpioPin<T, direction::Input>
where
    T: pins::Trait,
{
    type Error = Void;

    fn is_high(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_high())
    }

    fn is_low(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_low())
    }
}

impl<T> InputPinAlpha for GpioPin<T, direction::Input>
where
    T: pins::Trait,
{
    type Error = Void;

    fn try_is_high(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_high())
    }

    fn try_is_low(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_low())
    }
}

impl<T> OutputPin for GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    type Error = Void;

    fn set_high(&mut self) -> Result<(), Self::Error> {
        // Call the inherent method defined above.
        Ok(self.set_high())
    }

    fn set_low(&mut self) -> Result<(), Self::Error> {
        // Call the inherent method defined above.
        Ok(self.set_low())
    }
}

impl<T> OutputPinAlpha for GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    type Error = Void;

    fn try_set_high(&mut self) -> Result<(), Self::Error> {
        // Call the inherent method defined above.
        Ok(self.set_high())
    }

    fn try_set_low(&mut self) -> Result<(), Self::Error> {
        // Call the inherent method defined above.
        Ok(self.set_low())
    }
}

impl<T> StatefulOutputPin for GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    fn is_set_high(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_set_high())
    }

    fn is_set_low(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_set_low())
    }
}

impl<T> StatefulOutputPinAlpha for GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    fn try_is_set_high(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_set_high())
    }

    fn try_is_set_low(&self) -> Result<bool, Self::Error> {
        // Call the inherent method defined above.
        Ok(self.is_set_low())
    }
}

impl<T> ToggleableOutputPin for GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    type Error = Void;

    fn toggle(&mut self) -> Result<(), Self::Error> {
        // Call the inherent method defined above.
        Ok(self.toggle())
    }
}

impl<T> ToggleableOutputPinAlpha for GpioPin<T, direction::Output>
where
    T: pins::Trait,
{
    type Error = Void;

    fn try_toggle(&mut self) -> Result<(), Self::Error> {
        // Call the inherent method defined above.
        Ok(self.toggle())
    }
}

/// The voltage level of a pin
#[derive(Debug)]
pub enum Level {
    /// High voltage
    High,

    /// Low voltage
    Low,
}

fn set_high<T: pins::Trait>(registers: &Registers) {
    registers.set[T::PORT].write(|w| unsafe { w.setp().bits(T::MASK) });
}

fn set_low<T: pins::Trait>(registers: &Registers) {
    registers.clr[T::PORT].write(|w| unsafe { w.clrp().bits(T::MASK) });
}

fn is_high<T: pins::Trait>(registers: &Registers) -> bool {
    registers.pin[T::PORT].read().port().bits() & T::MASK == T::MASK
}

// For internal use only.
// Use the direction helpers of GpioPin<T, direction::Output> and GpioPin<T, direction::Dynamic>
// instead.
fn set_direction_output<T: pins::Trait>(registers: &Registers) {
    registers.dirset[T::PORT].write(|w| unsafe { w.dirsetp().bits(T::MASK) });
}

// For internal use only.
// Use the direction helpers of GpioPin<T, direction::Input> and GpioPin<T, direction::Dynamic>
// instead.
fn set_direction_input<T: pins::Trait>(registers: &Registers) {
    registers.dirclr[T::PORT].write(|w| unsafe { w.dirclrp().bits(T::MASK) });
}

/// This is an internal type that should be of no concern to users of this crate
pub struct Registers<'gpio> {
    dirset: &'gpio [DIRSET],
    dirclr: &'gpio [DIRCLR],
    pin: &'gpio [PIN],
    set: &'gpio [SET],
    clr: &'gpio [CLR],
    not: &'gpio [NOT],
}

impl<'gpio> Registers<'gpio> {
    /// Create a new instance of `Registers` from the provided register block
    ///
    /// If the reference to `RegisterBlock` is not exclusively owned by the
    /// caller, accessing all registers is still completely race-free, as long
    /// as the following rules are upheld:
    /// - Never write to `pin`, only use it for reading.
    /// - For all other registers, only set bits that no other callers are
    ///   setting.
    fn new(gpio: &'gpio pac::gpio::RegisterBlock) -> Self {
        #[cfg(feature = "82x")]
        {
            use core::slice;

            Self {
                dirset: slice::from_ref(&gpio.dirset0),
                dirclr: slice::from_ref(&gpio.dirclr0),
                pin: slice::from_ref(&gpio.pin0),
                set: slice::from_ref(&gpio.set0),
                clr: slice::from_ref(&gpio.clr0),
                not: slice::from_ref(&gpio.not0),
            }
        }

        #[cfg(feature = "845")]
        Self {
            dirset: &gpio.dirset,
            dirclr: &gpio.dirclr,
            pin: &gpio.pin,
            set: &gpio.set,
            clr: &gpio.clr,
            not: &gpio.not,
        }
    }
}

/// Contains types to indicate the direction of GPIO pins
///
/// Please refer to [`GpioPin`] for documentation on how these types are used.
///
/// [`GpioPin`]: ../struct.GpioPin.html
pub mod direction {
    use crate::pins;

    use super::{Level, Registers};

    /// Implemented by types that indicate GPIO pin direction
    ///
    /// The [`GpioPin`] type uses this trait as a bound for its type parameter.
    /// This is done for documentation purposes, to clearly show which types can
    /// be used for this parameter. Other than that, this trait should not be
    /// relevant to users of this crate.
    ///
    /// [`GpioPin`]: ../struct.GpioPin.html
    pub trait Direction {
        /// The argument of the `switch` method
        type SwitchArg;

        /// Switch a pin to this direction
        ///
        /// This method is for internal use only. Any changes to it won't be
        /// considered breaking changes.
        fn switch<T: pins::Trait>(_: &Registers, _: Self::SwitchArg) -> Self;
    }

    /// Marks a GPIO pin as being configured for input
    ///
    /// This type is used as a type parameter of [`GpioPin`]. Please refer to
    /// the documentation there to see how this type is used.
    ///
    /// [`GpioPin`]: ../struct.GpioPin.html
    pub struct Input(());

    impl Direction for Input {
        type SwitchArg = ();

        fn switch<T: pins::Trait>(
            registers: &Registers,
            _: Self::SwitchArg,
        ) -> Self {
            super::set_direction_input::<T>(registers);
            Self(())
        }
    }

    /// Marks a GPIO pin as being configured for output
    ///
    /// This type is used as a type parameter of [`GpioPin`]. Please refer to
    /// the documentation there to see how this type is used.
    ///
    /// [`GpioPin`]: ../struct.GpioPin.html
    pub struct Output(());

    impl Direction for Output {
        type SwitchArg = Level;

        fn switch<T: pins::Trait>(
            registers: &Registers,
            initial: Level,
        ) -> Self {
            // First set the output level, before we switch the mode.
            match initial {
                Level::High => super::set_high::<T>(registers),
                Level::Low => super::set_low::<T>(registers),
            }

            // Now that the output level is configured, we can safely switch to
            // output mode, without risking an undesired signal between now and
            // the first call to `set_high`/`set_low`.
            super::set_direction_output::<T>(&registers);

            Self(())
        }
    }

    /// Marks a GPIO pin as being run-time configurable for in/output
    /// Initial direction is Output
    ///
    /// This type is used as a type parameter of [`GpioPin`]. Please refer to
    /// the documentation there to see how this type is used.
    ///
    /// [`GpioPin`]: ../struct.GpioPin.html
    pub struct Dynamic {
        pub(super) current_direction: pins::DynamicPinDirection,
    }

    /// Error that can be thrown by operations on a Dynamic pin
    #[derive(Copy, Clone)]
    pub enum DynamicPinErr {
        /// you called a function that is not applicable to the pin's current direction
        WrongDirection,
    }

    impl Direction for Dynamic {
        type SwitchArg = (Level, pins::DynamicPinDirection);

        fn switch<T: pins::Trait>(
            registers: &Registers,
            initial: Self::SwitchArg,
        ) -> Self {
            let (level, current_direction) = initial;

            // First set the output level, before we switch the mode.
            match level {
                Level::High => super::set_high::<T>(registers),
                Level::Low => super::set_low::<T>(registers),
            }

            match current_direction {
                pins::DynamicPinDirection::Input => {
                    // Now that the output level is configured, we can safely switch to
                    // output mode, without risking an undesired signal between now and
                    // the first call to `set_high`/`set_low`.
                    super::set_direction_input::<T>(registers);
                }
                pins::DynamicPinDirection::Output => {
                    // Now that the output level is configured, we can safely switch to
                    // output mode, without risking an undesired signal between now and
                    // the first call to `set_high`/`set_low`.
                    super::set_direction_output::<T>(registers);
                }
            }

            Self { current_direction }
        }
    }
}
