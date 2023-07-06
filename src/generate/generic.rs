use core::marker;

/// Raw register type (`u8`, `u16`, `u32`, ...)
pub trait RawReg:
    Copy
    + Default
    + From<bool>
    + core::ops::BitOr<Output = Self>
    + core::ops::BitAnd<Output = Self>
    + core::ops::BitOrAssign
    + core::ops::BitAndAssign
    + core::ops::Not<Output = Self>
    + core::ops::Shl<u8, Output = Self>
{
    /// Mask for bits of width `WI`
    fn mask<const WI: u8>() -> Self;
    /// Mask for bits of width 1
    fn one() -> Self;
}

macro_rules! raw_reg {
    ($U:ty, $size:literal, $mask:ident) => {
        impl RawReg for $U {
            #[inline(always)]
            fn mask<const WI: u8>() -> Self {
                $mask::<WI>()
            }
            #[inline(always)]
            fn one() -> Self {
                1
            }
        }
        const fn $mask<const WI: u8>() -> $U {
            <$U>::MAX >> ($size - WI)
        }
        impl FieldSpec for $U {
            type Ux = $U;
        }
    };
}

raw_reg!(u8, 8, mask_u8);
raw_reg!(u16, 16, mask_u16);
raw_reg!(u32, 32, mask_u32);
raw_reg!(u64, 64, mask_u64);

/// Raw register type
pub trait RegisterSpec {
    /// Raw register type (`u8`, `u16`, `u32`, ...).
    type Ux: RawReg;
}

/// Raw field type
pub trait FieldSpec: Sized {
    /// Raw field type (`u8`, `u16`, `u32`, ...).
    type Ux: Copy + PartialEq + From<Self>;
}

/// Trait implemented by readable registers to enable the `read` method.
///
/// Registers marked with `Writable` can be also be `modify`'ed.
pub trait Readable: RegisterSpec {}

/// Trait implemented by writeable registers.
///
/// This enables the  `write`, `write_with_zero` and `reset` methods.
///
/// Registers marked with `Readable` can be also be `modify`'ed.
pub trait Writable: RegisterSpec {
    /// Specifies the register bits that are not changed if you pass `1` and are changed if you pass `0`
    const ZERO_TO_MODIFY_FIELDS_BITMAP: Self::Ux;

    /// Specifies the register bits that are not changed if you pass `0` and are changed if you pass `1`
    const ONE_TO_MODIFY_FIELDS_BITMAP: Self::Ux;
}

/// Reset value of the register.
///
/// This value is the initial value for the `write` method. It can also be directly written to the
/// register by using the `reset` method.
pub trait Resettable: RegisterSpec {
    /// Reset value of the register.
    const RESET_VALUE: Self::Ux;

    /// Reset value of the register.
    #[inline(always)]
    fn reset_value() -> Self::Ux {
        Self::RESET_VALUE
    }
}

/// This structure provides volatile access to registers.
#[repr(transparent)]
pub struct Reg<REG: RegisterSpec> {
    register: vcell::VolatileCell<REG::Ux>,
    _marker: marker::PhantomData<REG>,
}

unsafe impl<REG: RegisterSpec> Send for Reg<REG> where REG::Ux: Send {}

impl<REG: RegisterSpec> Reg<REG> {
    /// Returns the underlying memory address of register.
    ///
    /// ```ignore
    /// let reg_ptr = periph.reg.as_ptr();
    /// ```
    #[inline(always)]
    pub fn as_ptr(&self) -> *mut REG::Ux {
        self.register.as_ptr()
    }
}

impl<REG: Readable> Reg<REG> {
    /// Reads the contents of a `Readable` register.
    ///
    /// You can read the raw contents of a register by using `bits`:
    /// ```ignore
    /// let bits = periph.reg.read().bits();
    /// ```
    /// or get the content of a particular field of a register:
    /// ```ignore
    /// let reader = periph.reg.read();
    /// let bits = reader.field1().bits();
    /// let flag = reader.field2().bit_is_set();
    /// ```
    #[inline(always)]
    pub fn read(&self) -> R<REG> {
        R::new(self.register.get())
    }
}

impl<REG: Resettable + Writable> Reg<REG> {
    /// Writes the reset value to `Writable` register.
    ///
    /// Resets the register to its initial state.
    #[inline(always)]
    pub fn reset(&self) -> W<REG> {
        self.register.set(REG::RESET_VALUE);
        W::new(REG::RESET_VALUE)
    }

    /// Writes bits to a `Writable` register.
    ///
    /// You can write raw bits into a register:
    /// ```ignore
    /// periph.reg.write(|w| unsafe { w.bits(rawbits) });
    /// ```
    /// or write only the fields you need:
    /// ```ignore
    /// periph.reg.write(|w| w
    ///     .field1().bits(newfield1bits)
    ///     .field2().set_bit()
    ///     .field3().variant(VARIANT)
    /// );
    /// ```
    /// or an alternative way of saying the same:
    /// ```ignore
    /// periph.reg.write(|w| {
    ///     w.field1().bits(newfield1bits);
    ///     w.field2().set_bit();
    ///     w.field3().variant(VARIANT)
    /// });
    /// ```
    /// In the latter case, other fields will be set to their reset value.
    #[inline(always)]
    pub fn write<F>(&self, f: F) -> W<REG>
    where
        F: FnOnce(W<REG>) -> W<REG>,
    {
        let w = f(W::new(
            REG::RESET_VALUE & !REG::ONE_TO_MODIFY_FIELDS_BITMAP
                | REG::ZERO_TO_MODIFY_FIELDS_BITMAP,
        ));
        self.register.set(w.bits);
        w
    }
}

impl<REG: Writable> Reg<REG> {
    /// Writes 0 to a `Writable` register.
    ///
    /// Similar to `write`, but unused bits will contain 0.
    ///
    /// # Safety
    ///
    /// Unsafe to use with registers which don't allow to write 0.
    #[inline(always)]
    pub unsafe fn write_with_zero<F>(&self, f: F)
    where
        F: FnOnce(W<REG>) -> W<REG>,
    {
        self.register.set(f(W::new(REG::Ux::default())).bits);
    }
}

impl<REG: Readable + Writable> Reg<REG> {
    /// Modifies the contents of the register by reading and then writing it.
    ///
    /// E.g. to do a read-modify-write sequence to change parts of a register:
    /// ```ignore
    /// periph.reg.modify(|r, w| unsafe { w.bits(
    ///    r.bits() | 3
    /// ) });
    /// ```
    /// or
    /// ```ignore
    /// periph.reg.modify(|_, w| w
    ///     .field1().bits(newfield1bits)
    ///     .field2().set_bit()
    ///     .field3().variant(VARIANT)
    /// );
    /// ```
    /// or an alternative way of saying the same:
    /// ```ignore
    /// periph.reg.modify(|_, w| {
    ///     w.field1().bits(newfield1bits);
    ///     w.field2().set_bit();
    ///     w.field3().variant(VARIANT)
    /// });
    /// ```
    /// Other fields will have the value they had before the call to `modify`.
    #[inline(always)]
    pub fn modify<F>(&self, f: F) -> W<REG>
    where
        F: FnOnce(R<REG>, W<REG>) -> W<REG>,
    {
        let bits = self.register.get();
        let w = f(
            R::new(bits),
            W::new(bits & !REG::ONE_TO_MODIFY_FIELDS_BITMAP | REG::ZERO_TO_MODIFY_FIELDS_BITMAP),
        );
        self.register.set(w.bits);
        w
    }
}

/// Register reader.
///
/// Result of the `read` methods of registers. Also used as a closure argument in the `modify`
/// method.
pub type R<REG> = RRaw<REG>;

#[doc(hidden)]
#[derive(Copy)]
pub struct RRaw<REG: RegisterSpec> {
    pub(crate) bits: REG::Ux,
    _reg: marker::PhantomData<REG>,
}

impl<REG: RegisterSpec> Clone for RRaw<REG> {
    fn clone(&self) -> Self {
        Self::new(self.bits)
    }
}

impl<REG: RegisterSpec> RRaw<REG> {
    #[inline(always)]
    const fn new(bits: REG::Ux) -> Self {
        Self {
            bits,
            _reg: marker::PhantomData,
        }
    }
}

impl<REG: RegisterSpec> R<REG> {
    /// Reads raw bits from register.
    #[inline(always)]
    pub fn bits(&self) -> REG::Ux {
        self.bits
    }
}

impl<REG: RegisterSpec, FI> PartialEq<FI> for R<REG>
where
    REG::Ux: PartialEq,
    FI: Copy,
    REG::Ux: From<FI>,
{
    #[inline(always)]
    fn eq(&self, other: &FI) -> bool {
        self.bits.eq(&REG::Ux::from(*other))
    }
}

/// Register writer.
///
/// Used as an argument to the closures in the `write` and `modify` methods of the register.
pub type W<REG> = WRaw<REG>;

#[doc(hidden)]
#[derive(Copy)]
pub struct WRaw<REG: RegisterSpec> {
    ///Writable bits
    pub(crate) bits: REG::Ux,
    _reg: marker::PhantomData<REG>,
}

impl<REG: RegisterSpec> Clone for WRaw<REG> {
    fn clone(&self) -> Self {
        Self::new(self.bits)
    }
}

impl<REG: RegisterSpec> WRaw<REG> {
    #[inline(always)]
    const fn new(bits: REG::Ux) -> Self {
        Self {
            bits,
            _reg: marker::PhantomData,
        }
    }
}

#[doc(hidden)]
pub struct FieldReaderRaw<FI = u8>
where
    FI: FieldSpec,
{
    pub(crate) bits: FI::Ux,
    _reg: marker::PhantomData<FI>,
}

impl<FI: FieldSpec> FieldReaderRaw<FI> {
    /// Creates a new instance of the reader.
    #[allow(unused)]
    #[inline(always)]
    pub(crate) fn new(bits: FI::Ux) -> Self {
        Self {
            bits,
            _reg: marker::PhantomData,
        }
    }
}

#[doc(hidden)]
pub struct BitReaderRaw<FI = bool> {
    pub(crate) bits: bool,
    _reg: marker::PhantomData<FI>,
}

impl<FI> BitReaderRaw<FI> {
    /// Creates a new instance of the reader.
    #[allow(unused)]
    #[inline(always)]
    pub(crate) fn new(bits: bool) -> Self {
        Self {
            bits,
            _reg: marker::PhantomData,
        }
    }
}

/// Field reader.
///
/// Result of the `read` methods of fields.
pub type FieldReader<FI = u8> = FieldReaderRaw<FI>;

/// Bit-wise field reader
pub type BitReader<FI = bool> = BitReaderRaw<FI>;

impl<FI: FieldSpec> FieldReader<FI> {
    /// Reads raw bits from field.
    #[inline(always)]
    pub fn bits(&self) -> FI::Ux {
        self.bits
    }
}

impl<FI> PartialEq<FI> for FieldReader<FI>
where
    FI: FieldSpec + Copy,
{
    #[inline(always)]
    fn eq(&self, other: &FI) -> bool {
        self.bits.eq(&FI::Ux::from(*other))
    }
}

impl<FI> PartialEq<FI> for BitReader<FI>
where
    FI: Copy,
    bool: From<FI>,
{
    #[inline(always)]
    fn eq(&self, other: &FI) -> bool {
        self.bits.eq(&bool::from(*other))
    }
}

impl<FI> BitReader<FI> {
    /// Value of the field as raw bits.
    #[inline(always)]
    pub fn bit(&self) -> bool {
        self.bits
    }
    /// Returns `true` if the bit is clear (0).
    #[inline(always)]
    pub fn bit_is_clear(&self) -> bool {
        !self.bit()
    }
    /// Returns `true` if the bit is set (1).
    #[inline(always)]
    pub fn bit_is_set(&self) -> bool {
        self.bit()
    }
}

#[doc(hidden)]
pub struct Safe;
#[doc(hidden)]
pub struct Unsafe;

#[doc(hidden)]
pub struct FieldWriterRaw<REG, const WI: u8, const O: u8, FI = u8, Safety = Unsafe>
where
    REG: Writable + RegisterSpec,
    FI: FieldSpec,
{
    pub(crate) w: W<REG>,
    _field: marker::PhantomData<(FI, Safety)>,
}

impl<REG, const WI: u8, const O: u8, FI, Safety> FieldWriterRaw<REG, WI, O, FI, Safety>
where
    REG: Writable + RegisterSpec,
    FI: FieldSpec,
{
    /// Creates a new instance of the writer
    #[allow(unused)]
    #[inline(always)]
    pub(crate) fn new(w: W<REG>) -> Self {
        Self {
            w,
            _field: marker::PhantomData,
        }
    }
}

#[doc(hidden)]
pub struct BitWriterRaw<REG, const O: u8, FI = bool, M = BitM>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    pub(crate) w: W<REG>,
    _field: marker::PhantomData<(FI, M)>,
}

impl<REG, const O: u8, FI, M> BitWriterRaw<REG, O, FI, M>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    /// Creates a new instance of the writer
    #[allow(unused)]
    #[inline(always)]
    pub(crate) fn new(w: W<REG>) -> Self {
        Self {
            w,
            _field: marker::PhantomData,
        }
    }
}

/// Write field Proxy with unsafe `bits`
pub type FieldWriter<REG, const WI: u8, const O: u8, FI = u8> =
    FieldWriterRaw<REG, WI, O, FI, Unsafe>;
/// Write field Proxy with safe `bits`
pub type FieldWriterSafe<REG, const WI: u8, const O: u8, FI = u8> =
    FieldWriterRaw<REG, WI, O, FI, Safe>;

impl<REG, const WI: u8, const OF: u8, FI> FieldWriter<REG, WI, OF, FI>
where
    REG: Writable + RegisterSpec,
    FI: FieldSpec,
{
    /// Field width
    pub const WIDTH: u8 = WI;
}

impl<REG, const WI: u8, const OF: u8, FI> FieldWriterSafe<REG, WI, OF, FI>
where
    REG: Writable + RegisterSpec,
    FI: FieldSpec,
{
    /// Field width
    pub const WIDTH: u8 = WI;
}

macro_rules! bit_proxy {
    ($writer:ident, $mwv:ident) => {
        #[doc(hidden)]
        pub struct $mwv;

        /// Bit-wise write field proxy
        pub type $writer<REG, const O: u8, FI = bool> = BitWriterRaw<REG, O, FI, $mwv>;

        impl<REG, const OF: u8, FI> $writer<REG, OF, FI>
        where
            REG: Writable + RegisterSpec,
            bool: From<FI>,
        {
            /// Field width
            pub const WIDTH: u8 = 1;
        }
    };
}

macro_rules! impl_bit_proxy {
    ($writer:ident) => {
        impl<REG, const OF: u8, FI> $writer<REG, OF, FI>
        where
            REG: Writable + RegisterSpec,
            bool: From<FI>,
        {
            /// Writes bit to the field
            #[inline(always)]
            pub fn bit(mut self, value: bool) -> W<REG> {
                self.w.bits &= !(REG::Ux::one() << OF);
                self.w.bits |= (REG::Ux::from(value) & REG::Ux::one()) << OF;
                self.w
            }
            /// Writes `variant` to the field
            #[inline(always)]
            pub fn variant(self, variant: FI) -> W<REG> {
                self.bit(bool::from(variant))
            }
        }
    };
}

bit_proxy!(BitWriter, BitM);
bit_proxy!(BitWriter1S, Bit1S);
bit_proxy!(BitWriter0C, Bit0C);
bit_proxy!(BitWriter1C, Bit1C);
bit_proxy!(BitWriter0S, Bit0S);
bit_proxy!(BitWriter1T, Bit1T);
bit_proxy!(BitWriter0T, Bit0T);

impl<REG, const WI: u8, const OF: u8, FI> FieldWriter<REG, WI, OF, FI>
where
    REG: Writable + RegisterSpec,
    FI: FieldSpec,
    REG::Ux: From<FI::Ux>,
{
    /// Writes raw bits to the field
    ///
    /// # Safety
    ///
    /// Passing incorrect value can cause undefined behaviour. See reference manual
    #[inline(always)]
    pub unsafe fn bits(mut self, value: FI::Ux) -> W<REG> {
        self.w.bits &= !(REG::Ux::mask::<WI>() << OF);
        self.w.bits |= (REG::Ux::from(value) & REG::Ux::mask::<WI>()) << OF;
        self.w
    }
    /// Writes `variant` to the field
    #[inline(always)]
    pub fn variant(self, variant: FI) -> W<REG> {
        unsafe { self.bits(FI::Ux::from(variant)) }
    }
}
impl<REG, const WI: u8, const OF: u8, FI> FieldWriterSafe<REG, WI, OF, FI>
where
    REG: Writable + RegisterSpec,
    FI: FieldSpec,
    REG::Ux: From<FI::Ux>,
{
    /// Writes raw bits to the field
    #[inline(always)]
    pub fn bits(mut self, value: FI::Ux) -> W<REG> {
        self.w.bits &= !(REG::Ux::mask::<WI>() << OF);
        self.w.bits |= (REG::Ux::from(value) & REG::Ux::mask::<WI>()) << OF;
        self.w
    }
    /// Writes `variant` to the field
    #[inline(always)]
    pub fn variant(self, variant: FI) -> W<REG> {
        self.bits(FI::Ux::from(variant))
    }
}

impl_bit_proxy!(BitWriter);
impl_bit_proxy!(BitWriter1S);
impl_bit_proxy!(BitWriter0C);
impl_bit_proxy!(BitWriter1C);
impl_bit_proxy!(BitWriter0S);
impl_bit_proxy!(BitWriter1T);
impl_bit_proxy!(BitWriter0T);

impl<REG, const OF: u8, FI> BitWriter<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    /// Sets the field bit
    #[inline(always)]
    pub fn set_bit(mut self) -> W<REG> {
        self.w.bits |= REG::Ux::one() << OF;
        self.w
    }
    /// Clears the field bit
    #[inline(always)]
    pub fn clear_bit(mut self) -> W<REG> {
        self.w.bits &= !(REG::Ux::one() << OF);
        self.w
    }
}

impl<REG, const OF: u8, FI> BitWriter1S<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    /// Sets the field bit
    #[inline(always)]
    pub fn set_bit(mut self) -> W<REG> {
        self.w.bits |= REG::Ux::one() << OF;
        self.w
    }
}

impl<REG, const OF: u8, FI> BitWriter0C<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    /// Clears the field bit
    #[inline(always)]
    pub fn clear_bit(mut self) -> W<REG> {
        self.w.bits &= !(REG::Ux::one() << OF);
        self.w
    }
}

impl<REG, const OF: u8, FI> BitWriter1C<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    ///Clears the field bit by passing one
    #[inline(always)]
    pub fn clear_bit_by_one(mut self) -> W<REG> {
        self.w.bits |= REG::Ux::one() << OF;
        self.w
    }
}

impl<REG, const OF: u8, FI> BitWriter0S<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    ///Sets the field bit by passing zero
    #[inline(always)]
    pub fn set_bit_by_zero(mut self) -> W<REG> {
        self.w.bits &= !(REG::Ux::one() << OF);
        self.w
    }
}

impl<REG, const OF: u8, FI> BitWriter1T<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    ///Toggle the field bit by passing one
    #[inline(always)]
    pub fn toggle_bit(mut self) -> W<REG> {
        self.w.bits |= REG::Ux::one() << OF;
        self.w
    }
}

impl<REG, const OF: u8, FI> BitWriter0T<REG, OF, FI>
where
    REG: Writable + RegisterSpec,
    bool: From<FI>,
{
    ///Toggle the field bit by passing zero
    #[inline(always)]
    pub fn toggle_bit(mut self) -> W<REG> {
        self.w.bits &= !(REG::Ux::one() << OF);
        self.w
    }
}
