use crate::mini_v8::MiniV8;
use crate::value::Value;
use std::error::Error as StdError;
use std::fmt;
use std::result::Result as StdResult;

pub type Result<'mv8, T> = StdResult<T, Error<'mv8>>;

#[derive(Debug)]
pub enum Error<'mv8> {
    /// A Rust value could not be converted to a JavaScript value.
    ToJsConversionError {
        /// Name of the Rust type that could not be converted.
        from: &'static str,
        /// Name of the JavaScript type that could not be created.
        to: &'static str,
    },
    /// A JavaScript value could not be converted to the expected Rust type.
    FromJsConversionError {
        /// Name of the JavaScript type that could not be converted.
        from: &'static str,
        /// Name of the Rust type that could not be created.
        to: &'static str,
    },
    /// A mutable callback has triggered JavaScript code that has called the same mutable callback
    /// again.
    ///
    /// This is an error because a mutable callback can only be borrowed mutably once.
    RecursiveMutCallback,
    /// An error specifying the variable that was called as a function was not a function.
    NotAFunction,
    /// A custom error that occurs during runtime.
    ///
    /// This can be used for returning user-defined errors from callbacks.
    ExternalError(Box<StdError + 'static>),
    /// An exception that occurred within the JavaScript environment.
    Value(Value<'mv8>),
}

impl<'mv8> Error<'mv8> {
    pub fn from_js_conversion(from: &'static str, to: &'static str) -> Error<'mv8> {
        Error::FromJsConversionError { from, to }
    }

    pub fn to_js_conversion(from: &'static str, to: &'static str) -> Error<'mv8> {
        Error::ToJsConversionError { from, to }
    }

    pub fn recursive_mut_callback() -> Error<'mv8> {
        Error::RecursiveMutCallback
    }

    pub fn not_a_function() -> Error<'mv8> {
        Error::NotAFunction
    }

    pub(crate) fn to_value(self, mv8: &'mv8 MiniV8) -> Value<'mv8> {
        match self {
            Error::Value(value) => value,
            Error::ToJsConversionError { .. } |
            Error::FromJsConversionError { .. } |
            Error::NotAFunction => {
                let object = mv8.create_object();
                let _ = object.set("name", "TypeError");
                let _ = object.set("message", self.to_string());
                Value::Object(object)
            },
            _ => {
                let object = mv8.create_object();
                let _ = object.set("name", "Error");
                let _ = object.set("message", self.to_string());
                Value::Object(object)
            },
        }
    }
}


impl<'mv8> StdError for Error<'mv8> {
    fn description(&self) -> &'static str {
        "JavaScript execution error"
    }
}

impl<'mv8> fmt::Display for Error<'mv8> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::ToJsConversionError { from, to } => {
                write!(fmt, "error converting {} to JavaScript {}", from, to)
            },
            Error::FromJsConversionError { from, to } => {
                write!(fmt, "error converting JavaScript {} to {}", from, to)
            },
            Error::RecursiveMutCallback => write!(fmt, "mutable callback called recursively"),
            Error::NotAFunction => write!(fmt, "tried to a call a non-function"),
            Error::ExternalError(ref err) => err.fmt(fmt),
            Error::Value(v) => write!(fmt, "JavaScript runtime error ({})", v.type_name()),
        }
    }
}
