use v8;
use serde_v8::from_v8;
use serde_v8::to_v8;
use serde::{Serialize, Deserialize};

pub fn init_js_platform() {
  let platform = v8::new_default_platform(0, false).make_shared();
  v8::V8::initialize_platform(platform);
  v8::V8::initialize();
}

pub fn run_js_fn<'a, T: Deserialize<'a>, U: Serialize> (js_fn: &str, opts: &U) -> T {
  let isolate = &mut v8::Isolate::new(Default::default());
  let handle_scope = &mut v8::HandleScope::new(isolate);
  let context = v8::Context::new(handle_scope);
  let scope = &mut v8::ContextScope::new(handle_scope, context);
  let code = v8::String::new(scope, js_fn).unwrap();
  let script = v8::Script::compile(scope, code, None).unwrap();
  let function = script.run(scope).unwrap();
  if !function.is_function() {
    panic!("Expected a function");
  }
  let cb = v8::Local::<v8::Function>::try_from(function).unwrap();
  let this = v8::undefined(scope).into();
  let args: Vec<v8::Local<v8::Value>> = vec![to_v8(scope, opts).expect("Unable to serialize")];
  let result = cb.call(scope, this, args.as_slice()).unwrap();
  let task: T = from_v8(scope, result).expect("Unable to deserialize");
  task
}
