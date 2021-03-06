use crate::*;
use std::ffi::c_void;
use std::mem::ManuallyDrop;
use std::sync::Once;
use std::{process, slice};
use std::panic::{AssertUnwindSafe, catch_unwind};

extern "C" {
    pub(crate) fn mv8_init(wrapper_func: *const c_void, drop_func: *const c_void);
    pub(crate) fn mv8_interface_new() -> Interface;
    pub(crate) fn mv8_interface_drop(_: Interface);
    pub(crate) fn mv8_interface_eval(
        _: Interface,
        source_data: *const u8,
        source_length: u32,
        name_data: *const u8,
        name_length: u32,
        line_offset: i32,
        column_offset: i32,
    ) -> TryCatchDesc;
    pub(crate) fn mv8_interface_terminate_execution(_: Interface);
    pub(crate) fn mv8_interface_global(_: Interface) -> ValuePtr;
    pub(crate) fn mv8_interface_set_data(_: Interface, slot: u32, data: *mut c_void);
    pub(crate) fn mv8_interface_get_data(_: Interface, slot: u32) -> *mut c_void;
    pub(crate) fn mv8_value_ptr_clone(_: Interface, value: ValuePtr) -> ValuePtr;
    pub(crate) fn mv8_value_ptr_drop(value_ptr: ValuePtr);
    pub(crate) fn mv8_string_new(_: Interface, data: *const u8, length: u32) -> ValuePtr;
    pub(crate) fn mv8_string_to_utf8_value(_: Interface, value: ValuePtr) -> Utf8Value;
    pub(crate) fn mv8_utf8_value_drop(utf8_value: Utf8Value);
    pub(crate) fn mv8_array_new(_: Interface) -> ValuePtr;
    pub(crate) fn mv8_array_get(_: Interface, array: ValuePtr, index: u32) -> TryCatchDesc;
    pub(crate) fn mv8_array_set(_: Interface, array: ValuePtr, index: u32, value: ValueDesc)
        -> TryCatchDesc;
    pub(crate) fn mv8_array_len(_: Interface, array: ValuePtr) -> u32;
    pub(crate) fn mv8_object_new(_: Interface) -> ValuePtr;
    pub(crate) fn mv8_object_get(_: Interface, object: ValuePtr, key: ValueDesc) -> TryCatchDesc;
    pub(crate) fn mv8_object_set(_: Interface, object: ValuePtr, key: ValueDesc, value: ValueDesc)
        -> TryCatchDesc;
    pub(crate) fn mv8_object_remove(_: Interface, object: ValuePtr, key: ValueDesc) -> TryCatchDesc;
    pub(crate) fn mv8_object_has(_: Interface, object: ValuePtr, key: ValueDesc) -> TryCatchDesc;
    pub(crate) fn mv8_object_keys(_: Interface, object: ValuePtr, include_inherited: u8)
        -> TryCatchDesc;
    pub(crate) fn mv8_coerce_boolean(_: Interface, value: ValueDesc) -> u8;
    pub(crate) fn mv8_coerce_number(_: Interface, value: ValueDesc) -> TryCatchDesc;
    pub(crate) fn mv8_coerce_string(_: Interface, value: ValueDesc) -> TryCatchDesc;
    pub(crate) fn mv8_function_create(_: Interface, func: *const c_void, func_size: u32)
        -> ValuePtr;
    pub(crate) fn mv8_function_call(
        _: Interface,
        func: ValuePtr,
        this_desc: ValueDesc,
        arg_descs: *const ValueDesc,
        arg_descs_len: i32,
    ) -> TryCatchDesc;
    pub(crate) fn mv8_function_call_new(
        _: Interface,
        func: ValuePtr,
        arg_descs: *const ValueDesc,
        arg_descs_len: i32,
    ) -> TryCatchDesc;
}

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) struct Interface(*const c_void);

unsafe impl Send for Interface {}

pub(crate) type ValuePtr = *const c_void;

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
pub(crate) enum ValueDescTag {
    Null,
    Undefined,
    Number,
    Boolean,
    Array,
    Function,
    Date,
    Object,
    String,
}

#[derive(Copy, Clone)]
#[repr(C)]
pub(crate) union ValueDescPayload {
    pub(crate) byte: u8,
    pub(crate) number: f64,
    pub(crate) value_ptr: ValuePtr,
}

#[repr(C)]
pub(crate) struct ValueDesc {
    pub(crate) payload: ValueDescPayload,
    pub(crate) tag: ValueDescTag,
}

impl Drop for ValueDesc {
    fn drop(&mut self) {
        match self.tag {
            ValueDescTag::String |
            ValueDescTag::Array |
            ValueDescTag::Function |
            ValueDescTag::Object => unsafe { mv8_value_ptr_drop(self.payload.value_ptr) },
            _ => {},
        }
    }
}

impl ValueDesc {
    pub(crate) fn new(tag: ValueDescTag, payload: ValueDescPayload) -> ValueDesc {
        ValueDesc { tag, payload }
    }
}

#[repr(C)]
pub(crate) struct TryCatchDesc {
    pub(crate) value_desc: ValueDesc,
    pub(crate) is_exception: u8,
}

#[repr(C)]
pub(crate) struct Utf8Value {
    pub(crate) data: *const u8,
    pub(crate) length: i32,
    src: *const c_void,
}

// A reference to a V8-owned value.
pub(crate) struct Ref<'mv8> {
    pub(crate) mv8: &'mv8 MiniV8,
    pub(crate) value_ptr: ValuePtr,
}

impl<'mv8> Ref<'mv8> {
    pub(crate) fn new(mv8: &MiniV8, value_ptr: ValuePtr) -> Ref {
        Ref { mv8, value_ptr }
    }

    pub(crate) fn from_value_desc(mv8: &MiniV8, desc: ValueDesc) -> Ref {
        let value_ptr = unsafe { desc.payload.value_ptr };
        // `Ref` has taken ownership of the `value_ptr`, so there's no need to run `ValueDesc`'s
        // drop:
        ManuallyDrop::new(desc);
        Ref { mv8, value_ptr }
    }
}

impl<'mv8> Clone for Ref<'mv8> {
    fn clone(&self) -> Ref<'mv8> {
        let value_ptr = unsafe { mv8_value_ptr_clone(self.mv8.interface, self.value_ptr) };
        Ref { mv8: self.mv8, value_ptr }
    }
}

impl<'mv8> Drop for Ref<'mv8> {
    fn drop(&mut self) {
        unsafe { mv8_value_ptr_drop(self.value_ptr); }
    }
}

pub(crate) fn ffi_init() {
    static INIT: Once = Once::new();
    INIT.call_once(|| unsafe { mv8_init(callback_wrapper as _, callback_drop as _) });
}

pub(crate) fn desc_to_result(mv8: &MiniV8, desc: TryCatchDesc) -> Result<Value> {
    let value = desc_to_value(mv8, desc.value_desc);
    if desc.is_exception == 0 { Ok(value) } else { Err(Error::Value(value)) }
}

pub(crate) fn desc_to_result_noval(mv8: &MiniV8, desc: TryCatchDesc) -> Result<()> {
    let is_exception = desc.is_exception == 1;
    if !is_exception { Ok(()) } else { Err(Error::Value(desc_to_value(mv8, desc.value_desc))) }
}

pub(crate) fn desc_to_result_val(mv8: &MiniV8, desc: TryCatchDesc) -> Result<ValueDesc> {
    let is_exception = desc.is_exception == 1;
    let desc = desc.value_desc;
    if !is_exception { Ok(desc) } else { Err(Error::Value(desc_to_value(mv8, desc))) }
}

pub(crate) fn desc_to_value(mv8: &MiniV8, desc: ValueDesc) -> Value {
    use ValueDescTag as VT;
    let value = match desc.tag {
        VT::Null => Value::Null,
        VT::Undefined => Value::Undefined,
        VT::Boolean => Value::Boolean(unsafe { desc.payload.byte != 0 }),
        VT::Number => Value::Number(unsafe { desc.payload.number }),
        VT::Date => Value::Date(unsafe { desc.payload.number }),
        VT::Array => Value::Array(Array(Ref::from_value_desc(mv8, desc))),
        VT::Function => Value::Function(Function(Ref::from_value_desc(mv8, desc))),
        VT::Object => Value::Object(Object(Ref::from_value_desc(mv8, desc))),
        VT::String => Value::String(String(Ref::from_value_desc(mv8, desc))),
    };

    value
}

pub(crate) fn value_to_desc<'mv8, 'a>(mv8: &'mv8 MiniV8, value: &'a Value<'mv8>) -> ValueDesc {
    fn ref_val(r: &Ref) -> ValuePtr {
        unsafe { mv8_value_ptr_clone(r.mv8.interface, r.value_ptr) }
    }

    use ValueDesc as V;
    use ValueDescTag as VT;
    use ValueDescPayload as VP;

    if let Some(r) = value.inner_ref() {
        if r.mv8.interface != mv8.interface {
            panic!("`Value` passed from one `MiniV8` instance to another");
        }
    }

    match *value {
        Value::Undefined => V::new(VT::Undefined, VP { byte: 0 }),
        Value::Null => V::new(VT::Null, VP { byte: 0 }),
        Value::Boolean(b) => V::new(VT::Boolean, VP { byte: if b { 1 } else { 0 } }),
        Value::Number(f) => V::new(VT::Number, VP { number: f }),
        Value::Date(f) => V::new(VT::Date, VP { number: f }),
        Value::Array(ref r) => V::new(VT::Array, VP { value_ptr: ref_val(&r.0) }),
        Value::Function(ref r) => V::new(VT::Function, VP { value_ptr: ref_val(&r.0) }),
        Value::Object(ref r) => V::new(VT::Object, VP { value_ptr: ref_val(&r.0) }),
        Value::String(ref r) => V::new(VT::String, VP { value_ptr: ref_val(&r.0) }),
    }
}

pub(crate) unsafe extern "C" fn callback_wrapper(
    interface: Interface,
    callback_ptr: *const c_void,
    this_desc: ValueDesc,
    arg_descs: *const ValueDesc,
    arg_descs_len: i32,
) -> TryCatchDesc {
    let inner = || {
        let mv8 = MiniV8 { interface, is_top: false };
        let this = desc_to_value(&mv8, this_desc);
        let arg_descs = slice::from_raw_parts(arg_descs, arg_descs_len as usize);
        // We take ownership of the `arg_descs` here, but C++ still manages the array and will free
        // it after this function ends:
        let args: Vec<Value> = arg_descs.iter()
            .map(|v| desc_to_value(&mv8, ValueDesc { payload: v.payload, tag: v.tag }))
            .collect();
        let args = Values::from_vec(args);

        let callback = callback_ptr as *mut Callback;
        let result = (*callback)(&mv8, this, args);
        let (is_exception, value) = match result {
            Ok(value) => (0, value),
            Err(value) => (1, value.to_value(&mv8)),
        };
        let value_desc = value_to_desc(&mv8, &value);
        // Ownership of the resultant `ValueDesc` is passed to C++:
        TryCatchDesc { is_exception, value_desc }
    };

    match catch_unwind(AssertUnwindSafe(inner)) {
        Ok(result) => result,
        Err(err) => {
            eprintln!("Panic during Rust function embedded in V8: {:?}", err);
            process::abort();
        },
    }
}

pub(crate) unsafe extern "C" fn callback_drop(callback: *mut Callback) {
    drop(Box::from_raw(callback));
}
