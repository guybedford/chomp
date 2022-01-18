use crate::engines::CmdOp;
use std::collections::BTreeMap;
use crate::chompfile::ChompTaskMaybeTemplatedNoDefault;
use anyhow::{anyhow, Error, Result};
use serde_v8::from_v8;
use serde_v8::to_v8;
use v8;

pub fn init_js_platform() {
  let platform = v8::new_default_platform(0, false).make_shared();
  v8::V8::initialize_platform(platform);
  v8::V8::initialize();
}

pub fn run_js_batcher<'a>(
  js_fn: &str,
  name: &str,
  queue: &Vec<CmdOp>,
  batch: &Vec<CmdOp>,
  running: &Vec<CmdOp>
) -> Result<Option<(Vec<CmdOp>, Vec<CmdOp>, usize)>> {
  let isolate = &mut v8::Isolate::new(Default::default());
  let handle_scope = &mut v8::HandleScope::new(isolate);
  let context = v8::Context::new(handle_scope);
  let scope = &mut v8::ContextScope::new(handle_scope, context);
  let code = v8::String::new(scope, js_fn).unwrap();
  let tc_scope = &mut v8::TryCatch::new(scope);
  let resource_name = v8::String::new(tc_scope, name).unwrap().into();
  let source_map = v8::String::new(tc_scope, "").unwrap().into();
  let origin = v8::ScriptOrigin::new(
    tc_scope,
    resource_name,
    0,
    0,
    false,
    123,
    source_map,
    true,
    false,
    false,
  );
  match v8::Script::compile(tc_scope, code, Some(&origin)) {
    Some(script) => {
      let function = script.run(tc_scope).unwrap();
      if !function.is_function() {
        panic!("Expected a function");
      }
      let cb = v8::Local::<v8::Function>::try_from(function).unwrap();
      let len_key = v8::String::new(tc_scope, "length").unwrap().into();
      let len: v8::Local<v8::Number> = cb.get(tc_scope, len_key).unwrap().try_into().unwrap();
      let this = v8::undefined(tc_scope).into();
      let args: Vec<v8::Local<v8::Value>> = vec![
        to_v8(tc_scope, queue).expect("Unable to serialize batcher call"),
        to_v8(tc_scope, batch).expect("Unable to serialize batcher call"),
        to_v8(tc_scope, running).expect("Unable to serialize batcher call")
      ];
      let result = match cb.call(tc_scope, this, args.as_slice()) {
        Some(result) => result,
        None => return Err(v8_exception(tc_scope)),
      };

      let result: Option<(Vec<CmdOp>, Vec<CmdOp>, usize)> = from_v8(tc_scope, result).expect("Unable to deserialize batch due to invalid structure");
      Ok(result)
    }
    None => Err(v8_exception(tc_scope)),
  }
}

pub fn run_js_tpl<'a>(
  js_fn: &str,
  name: &str,
  task: &ChompTaskMaybeTemplatedNoDefault,
  global_env: &BTreeMap<String, String>,
) -> Result<Vec<ChompTaskMaybeTemplatedNoDefault>> {
  let isolate = &mut v8::Isolate::new(Default::default());
  let handle_scope = &mut v8::HandleScope::new(isolate);
  let context = v8::Context::new(handle_scope);
  let scope = &mut v8::ContextScope::new(handle_scope, context);
  let code = v8::String::new(scope, js_fn).unwrap();
  let tc_scope = &mut v8::TryCatch::new(scope);
  let resource_name = v8::String::new(tc_scope, name).unwrap().into();
  let source_map = v8::String::new(tc_scope, "").unwrap().into();
  let origin = v8::ScriptOrigin::new(
    tc_scope,
    resource_name,
    0,
    0,
    false,
    123,
    source_map,
    true,
    false,
    false,
  );
  match v8::Script::compile(tc_scope, code, Some(&origin)) {
    Some(script) => {
      let function = script.run(tc_scope).unwrap();
      if !function.is_function() {
        panic!("Expected a function");
      }
      let cb = v8::Local::<v8::Function>::try_from(function).unwrap();
      let len_key = v8::String::new(tc_scope, "length").unwrap().into();
      let len: v8::Local<v8::Number> = cb.get(tc_scope, len_key).unwrap().try_into().unwrap();
      let this = v8::undefined(tc_scope).into();
      let args: Vec<v8::Local<v8::Value>> = if len.uint32_value(tc_scope).unwrap() == 2 {
        vec![to_v8(tc_scope, task).expect("Unable to serialize template params"), to_v8(tc_scope, global_env).expect("Unable to serialize global env")]
      } else {
        vec![to_v8(tc_scope, task).expect("Unable to serialize template params")]
      };
      let result = match cb.call(tc_scope, this, args.as_slice()) {
        Some(result) => result,
        None => return Err(v8_exception(tc_scope)),
      };
      let task: Vec<ChompTaskMaybeTemplatedNoDefault> = from_v8(tc_scope, result).expect("Unable to deserialize template task list due to invalid structure");
      Ok(task)
    }
    None => Err(v8_exception(tc_scope)),
  }
}

fn v8_exception<'a>(scope: &mut v8::TryCatch<v8::HandleScope>) -> Error {
  let exception = scope.exception().unwrap();
  if is_instance_of_error(scope, exception) {
    let exception: v8::Local<v8::Object> = exception.try_into().unwrap();

    let stack = get_property(scope, exception, "stack");
    let stack: Option<v8::Local<v8::String>> = stack.and_then(|s| s.try_into().ok());
    let stack = stack.map(|s| s.to_rust_string_lossy(scope));
    anyhow!("JS error: {}", stack.unwrap())
  } else {
    anyhow!("JS error: {}", exception.to_rust_string_lossy(scope))
  }
}

fn get_property<'a>(
  scope: &mut v8::HandleScope<'a>,
  object: v8::Local<v8::Object>,
  key: &str,
) -> Option<v8::Local<'a, v8::Value>> {
  let key = v8::String::new(scope, key).unwrap();
  object.get(scope, key.into())
}

fn is_instance_of_error<'s>(scope: &mut v8::HandleScope<'s>, value: v8::Local<v8::Value>) -> bool {
  if !value.is_object() {
    return false;
  }
  let message = v8::String::empty(scope);
  let error_prototype = v8::Exception::error(scope, message)
    .to_object(scope)
    .unwrap()
    .get_prototype(scope)
    .unwrap();
  let mut maybe_prototype = value.to_object(scope).unwrap().get_prototype(scope);
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

// fn exception_to_err_result<'s, T>(
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
