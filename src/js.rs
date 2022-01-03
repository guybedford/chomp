use v8;
use serde_v8::from_v8;
use serde_v8::to_v8;
use serde::{Serialize, Deserialize};
use anyhow::{Error,Result};

pub fn init_js_platform() {
  let platform = v8::new_default_platform(0, false).make_shared();
  v8::V8::initialize_platform(platform);
  v8::V8::initialize();
}

pub fn run_js_fn<'a, T: Deserialize<'a>, U: Serialize> (js_fn: &str, opts: &U) -> Result<T> {
  let isolate = &mut v8::Isolate::new(Default::default());
  let handle_scope = &mut v8::HandleScope::new(isolate);
  let context = v8::Context::new(handle_scope);
  let scope = &mut v8::ContextScope::new(handle_scope, context);
  let code = v8::String::new(scope, js_fn).unwrap();
  let tc_scope = &mut v8::TryCatch::new(scope);
  match v8::Script::compile(tc_scope, code, None) {
    Some(script) => {
      let function = script.run(tc_scope).unwrap();
      if !function.is_function() {
        panic!("Expected a function");
      }
      let cb = v8::Local::<v8::Function>::try_from(function).unwrap();
      let this = v8::undefined(tc_scope).into();
      let args: Vec<v8::Local<v8::Value>> = vec![to_v8(tc_scope, opts).expect("Unable to serialize")];
      let result = cb.call(tc_scope, this, args.as_slice()).unwrap();
      let task: T = from_v8(tc_scope, result).expect("Unable to deserialize");
      Ok(task)
    },
    None => {
      let exception = tc_scope.exception().unwrap();
      if is_instance_of_error(tc_scope, exception) {
        let exception: v8::Local<v8::Object> = exception.try_into().unwrap();

        let stack = get_property(scope, exception, "stack");
        let stack: Option<v8::Local<v8::String>> =
          stack.and_then(|s| s.try_into().ok());
        let stack = stack.map(|s| s.to_rust_string_lossy(scope));

        // Read an array of structured frames from error.__callSiteEvals.
        let frames_v8 = get_property(scope, exception, "__callSiteEvals");
        // Ignore non-array values
        let frames_v8: Option<v8::Local<v8::Array>> =
          frames_v8.and_then(|a| a.try_into().ok());

        // Convert them into Vec<JsStackFrame>
        let frames: Vec<JsStackFrame> = match frames_v8 {
          Some(frames_v8) => serde_v8::from_v8(scope, frames_v8.into()).unwrap(),
          None => vec![],
        };
        println!("{}", exception.to_rust_string_lossy(tc_scope));
        panic!("ERROR");
      }
      else {
        Err(exception.to_rust_string_lossy(tc_scope))
      }
    }
  }
}

#[derive(Debug, PartialEq, Clone)]
pub struct JsError {
  pub message: String,
  pub source_line: Option<String>,
  pub script_resource_name: Option<String>,
  pub line_number: Option<i64>,
  pub start_column: Option<i64>, // 0-based
  pub end_column: Option<i64>,   // 0-based
  pub frames: Vec<JsStackFrame>,
  pub stack: Option<String>,
}

#[derive(Debug, PartialEq, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JsStackFrame {
  pub type_name: Option<String>,
  pub function_name: Option<String>,
  pub method_name: Option<String>,
  pub file_name: Option<String>,
  pub line_number: Option<i64>,
  pub column_number: Option<i64>,
  pub eval_origin: Option<String>,
  // Warning! isToplevel has inconsistent snake<>camel case, "typo" originates in v8:
  // https://source.chromium.org/search?q=isToplevel&sq=&ss=chromium%2Fchromium%2Fsrc:v8%2F
  #[serde(rename = "isToplevel")]
  pub is_top_level: Option<bool>,
  pub is_eval: bool,
  pub is_native: bool,
  pub is_constructor: bool,
  pub is_async: bool,
  pub is_promise_all: bool,
  pub promise_index: Option<i64>,
}

fn get_property<'a>(
  scope: &mut v8::HandleScope<'a>,
  object: v8::Local<v8::Object>,
  key: &str,
) -> Option<v8::Local<'a, v8::Value>> {
  let key = v8::String::new(scope, key).unwrap();
  object.get(scope, key.into())
}

fn is_instance_of_error<'s>(
  scope: &mut v8::HandleScope<'s>,
  value: v8::Local<v8::Value>,
) -> bool {
  if !value.is_object() {
    return false;
  }
  let message = v8::String::empty(scope);
  let error_prototype = v8::Exception::error(scope, message)
    .to_object(scope)
    .unwrap()
    .get_prototype(scope)
    .unwrap();
  let mut maybe_prototype =
    value.to_object(scope).unwrap().get_prototype(scope);
  while let Some(prototype) = maybe_prototype {
    if prototype.strict_equals(error_prototype) {
      return true;
    }
    maybe_prototype = prototype
      .to_object(scope)
      .and_then(|o| o.get_prototype(scope));
  }
  false
}

// pub(crate) fn exception_to_err_result<'s, T>(
//   scope: &mut v8::HandleScope<'s>,
//   exception: v8::Local<v8::Value>,
//   in_promise: bool,
// ) -> Result<T, Error> {
//   let is_terminating_exception = scope.is_execution_terminating();
//   let mut exception = exception;

//   if is_terminating_exception {
//     // TerminateExecution was called. Cancel exception termination so that the
//     // exception can be created..
//     scope.cancel_terminate_execution();

//     // Maybe make a new exception object.
//     if exception.is_null_or_undefined() {
//       let message = v8::String::new(scope, "execution terminated").unwrap();
//       exception = v8::Exception::error(scope, message);
//     }
//   }

//   let mut js_error = JsError::from_v8_exception(scope, exception);
//   if in_promise {
//     js_error.message = format!(
//       "Uncaught (in promise) {}",
//       js_error.message.trim_start_matches("Uncaught ")
//     );
//   }

//   let state_rc = JsRuntime::state(scope);
//   let state = state_rc.borrow();
//   let js_error = (state.js_error_create_fn)(js_error);

//   if is_terminating_exception {
//     // Re-enable exception termination.
//     scope.terminate_execution();
//   }

//   Err(js_error)
// }
