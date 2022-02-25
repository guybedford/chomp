// Chomp Task Runner
// Copyright (C) 2022  Guy Bedford

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::chompfile::ChompTaskMaybeTemplatedJs;
use crate::engines::BatchCmd;
use crate::engines::CmdOp;
use anyhow::{anyhow, Error, Result};
use serde::Deserialize;
use serde_v8::from_v8;
use serde_v8::to_v8;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use v8;

pub struct ExtensionEnvironment {
    isolate: v8::OwnedIsolate,
    has_extensions: bool,
    global_context: v8::Global<v8::Context>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatcherResult {
    pub defer: Option<Vec<usize>>,
    pub exec: Option<Vec<BatchCmd>>,
    pub completion_map: Option<HashMap<usize, usize>>,
}

struct Extensions {
    pub tasks: Vec<ChompTaskMaybeTemplatedJs>,
    can_register: bool,
    includes: Vec<String>,
    templates: HashMap<String, v8::Global<v8::Function>>,
    batchers: Vec<(String, v8::Global<v8::Function>)>,
}

impl Extensions {
    fn new() -> Self {
        Extensions {
            can_register: true,
            tasks: Vec::new(),
            includes: Vec::new(),
            templates: HashMap::new(),
            batchers: Vec::new(),
        }
    }
}

pub fn init_js_platform() {
    let platform = v8::new_default_platform(0, false).make_shared();
    v8::V8::initialize_platform(platform);
    v8::V8::initialize();
}

fn chomp_log(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let mut msg = String::new();
    let len = args.length();
    let mut i = 0;
    while i < len {
        // TODO: better object logging - currently throws on objects
        let arg: v8::Local<v8::Value> = args.get(i).try_into().unwrap();
        if i > 0 {
            msg.push_str(", ");
        }
        msg.push_str(&arg.to_rust_string_lossy(scope));
        i = i + 1;
    }
    println!("{}", &msg);
}

fn chomp_include(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let include: String = {
        let tc_scope = &mut v8::TryCatch::new(scope);
        from_v8(tc_scope, args.get(0)).expect("Unable to register include")
    };
    let mut extension_env = scope
        .get_slot::<Rc<RefCell<Extensions>>>()
        .unwrap()
        .borrow_mut();
    if !extension_env.can_register {
        panic!("Chomp does not yet support dynamic includes.");
    }
    extension_env.includes.push(include);
}

fn chomp_register_task(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let task: ChompTaskMaybeTemplatedJs = {
        let tc_scope = &mut v8::TryCatch::new(scope);
        from_v8(tc_scope, args.get(0)).expect("Unable to register task")
    };
    let mut extension_env = scope
        .get_slot::<Rc<RefCell<Extensions>>>()
        .unwrap()
        .borrow_mut();
    if !extension_env.can_register {
        panic!("Chomp does not support dynamic task registration.");
    }
    extension_env.tasks.push(task);
}

fn chomp_register_template(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let name = args.get(0).to_string(scope).unwrap();
    let name_str = name.to_rust_string_lossy(scope);
    let tpl = v8::Local::<v8::Function>::try_from(args.get(1)).unwrap();
    let tpl_global = v8::Global::new(scope, tpl);

    let mut extension_env = scope
        .get_slot::<Rc<RefCell<Extensions>>>()
        .unwrap()
        .borrow_mut();
    if !extension_env.can_register {
        panic!("Chomp does not support dynamic template registration.");
    }
    extension_env.templates.insert(name_str, tpl_global);
}

fn chomp_register_batcher(
    scope: &mut v8::HandleScope,
    args: v8::FunctionCallbackArguments,
    mut _rv: v8::ReturnValue,
) {
    let name = args.get(0).to_string(scope).unwrap();
    let name_str = name.to_rust_string_lossy(scope);
    let batch = v8::Local::<v8::Function>::try_from(args.get(1)).unwrap();
    let batch_global = v8::Global::new(scope, batch);

    let mut extension_env = scope
        .get_slot::<Rc<RefCell<Extensions>>>()
        .unwrap()
        .borrow_mut();
    if !extension_env.can_register {
        panic!("Chomp does not support dynamic batcher registration.");
    }
    // remove any existing batcher by the same name
    if let Some(prev_batcher) = extension_env
        .batchers
        .iter()
        .position(|name| name.0 == name_str)
    {
        extension_env.batchers.remove(prev_batcher);
    }
    extension_env.batchers.push((name_str, batch_global));
}

impl ExtensionEnvironment {
    pub fn new(global_env: &BTreeMap<String, String>) -> Self {
        let mut isolate = v8::Isolate::new(Default::default());

        let global_context = {
            let mut handle_scope = v8::HandleScope::new(&mut isolate);
            let context = v8::Context::new(&mut handle_scope);
            let global = context.global(&mut handle_scope);

            let scope = &mut v8::ContextScope::new(&mut handle_scope, context);

            let chomp_key = v8::String::new(scope, "Chomp").unwrap();
            let chomp_val = v8::Object::new(scope);
            global.set(scope, chomp_key.into(), chomp_val.into());

            let console_key = v8::String::new(scope, "console").unwrap();
            let console_val = v8::Object::new(scope);
            global.set(scope, console_key.into(), console_val.into());

            let log_fn = v8::FunctionTemplate::new(scope, chomp_log)
                .get_function(scope)
                .unwrap();
            let log_key = v8::String::new(scope, "log").unwrap();
            console_val.set(scope, log_key.into(), log_fn.into());

            let version_key = v8::String::new(scope, "version").unwrap();
            let version_str = v8::String::new(scope, "0.1").unwrap();
            chomp_val.set(scope, version_key.into(), version_str.into());

            let task_fn = v8::FunctionTemplate::new(scope, chomp_register_task)
                .get_function(scope)
                .unwrap();
            let task_key = v8::String::new(scope, "registerTask").unwrap();
            chomp_val.set(scope, task_key.into(), task_fn.into());

            let tpl_fn = v8::FunctionTemplate::new(scope, chomp_register_template)
                .get_function(scope)
                .unwrap();
            let template_key = v8::String::new(scope, "registerTemplate").unwrap();
            chomp_val.set(scope, template_key.into(), tpl_fn.into());

            let batch_fn = v8::FunctionTemplate::new(scope, chomp_register_batcher)
                .get_function(scope)
                .unwrap();
            let batcher_key = v8::String::new(scope, "registerBatcher").unwrap();
            chomp_val.set(scope, batcher_key.into(), batch_fn.into());

            let include_fn = v8::FunctionTemplate::new(scope, chomp_include)
                .get_function(scope)
                .unwrap();
            let include_key = v8::String::new(scope, "addExtension").unwrap();
            chomp_val.set(scope, include_key.into(), include_fn.into());

            let env_key = v8::String::new(scope, "ENV").unwrap();
            let env_val = v8::Object::new(scope);
            global.set(scope, env_key.into(), env_val.into());

            for (key, value) in global_env {
                let env_key = v8::String::new(scope, key).unwrap();
                let env_key_val = v8::String::new(scope, value).unwrap();
                env_val.set(scope, env_key.into(), env_key_val.into());
            }

            v8::Global::new(scope, context)
        };

        let extensions = Extensions::new();
        isolate.set_slot(Rc::new(RefCell::new(extensions)));

        ExtensionEnvironment {
            isolate,
            has_extensions: false,
            global_context,
        }
    }

    fn handle_scope(&mut self) -> v8::HandleScope {
        v8::HandleScope::with_context(&mut self.isolate, self.global_context.clone())
    }

    pub fn get_tasks(&self) -> Vec<ChompTaskMaybeTemplatedJs> {
        self.isolate
            .get_slot::<Rc<RefCell<Extensions>>>()
            .unwrap()
            .borrow()
            .tasks
            .clone()
    }

    fn get_extensions(&self) -> &Rc<RefCell<Extensions>> {
        self.isolate.get_slot::<Rc<RefCell<Extensions>>>().unwrap()
    }

    pub fn add_extension(
        &mut self,
        extension_source: &str,
        filename: &str,
    ) -> Result<Option<Vec<String>>> {
        self.has_extensions = true;
        {
            let mut handle_scope = self.handle_scope();
            let code =
                v8::String::new(&mut handle_scope, &format!("{{{}}}", extension_source)).unwrap();
            let tc_scope = &mut v8::TryCatch::new(&mut handle_scope);
            let resource_name = v8::String::new(tc_scope, &filename).unwrap().into();
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
            let script = match v8::Script::compile(tc_scope, code, Some(&origin)) {
                Some(script) => script,
                None => return Err(v8_exception(tc_scope)),
            };
            match script.run(tc_scope) {
                Some(_) => {}
                None => return Err(v8_exception(tc_scope)),
            };
        }
        let mut extensions = self.get_extensions().borrow_mut();
        if extensions.includes.len() > 0 {
            Ok(Some(extensions.includes.drain(..).collect()))
        } else {
            Ok(None)
        }
    }

    pub fn seal_extensions(&mut self) {
        let mut extensions = self.get_extensions().borrow_mut();
        extensions.can_register = false;
    }

    pub fn run_template(
        &mut self,
        name: &str,
        task: &ChompTaskMaybeTemplatedJs,
    ) -> Result<Vec<ChompTaskMaybeTemplatedJs>> {
        let template = {
            let extensions = self.get_extensions().borrow();
            match extensions.templates.get(name) {
                Some(tpl) => Ok(tpl.clone()),
                None => {
                    if name == "babel"
                        || name == "cargo"
                        || name == "jspm"
                        || name == "npm"
                        || name == "prettier"
                        || name == "svelte"
                        || name == "swc"
                    {
                        if self.has_extensions {
                            Err(anyhow!("Template '{}' has not been registered. To include the core template, add \x1b[1m'chomp@0.1:{}'\x1b[0m to the extensions list:\x1b[36m\n\n  extensions = [..., 'chomp@0.1:{}']\n\n\x1b[0min the \x1b[1mchompfile.toml\x1b[0m.", &name, &name, &name))
                        } else {
                            Err(anyhow!("Template '{}' has not been registered. To include the core template, add:\x1b[36m\n\n  extensions = ['chomp@0.1:{}']\n\n\x1b[0mto the \x1b[1mchompfile.toml\x1b[0m.", &name, &name))
                        }
                    } else {
                        Err(anyhow!("Template '{}' has not been registered. Make sure it is included in the \x1b[1mchompfile.toml\x1b[0m extensions.", &name))
                    }
                }
            }
        }?;
        let cb = template.open(&mut self.isolate);

        let mut handle_scope = self.handle_scope();
        let tc_scope = &mut v8::TryCatch::new(&mut handle_scope);

        let this = v8::undefined(tc_scope).into();
        let args: Vec<v8::Local<v8::Value>> =
            vec![to_v8(tc_scope, task).expect("Unable to serialize template params")];
        let result = match cb.call(tc_scope, this, args.as_slice()) {
            Some(result) => result,
            None => return Err(v8_exception(tc_scope)),
        };
        let task: Vec<ChompTaskMaybeTemplatedJs> = from_v8(tc_scope, result)
            .expect("Unable to deserialize template task list due to invalid structure");
        Ok(task)
    }

    pub fn has_batchers(&self) -> bool {
        self.get_extensions().borrow().batchers.len() > 0
    }

    pub fn run_batcher(
        &mut self,
        idx: usize,
        batch: &HashSet<&CmdOp>,
        running: &HashSet<&BatchCmd>,
    ) -> Result<(BatcherResult, Option<usize>)> {
        let (name, batcher, batchers_len) = {
            let extensions = self.get_extensions().borrow();
            let (name, batcher) = extensions.batchers[idx].clone();
            (name, batcher, extensions.batchers.len())
        };
        let cb = batcher.open(&mut self.isolate);

        let mut handle_scope = self.handle_scope();
        let tc_scope = &mut v8::TryCatch::new(&mut handle_scope);

        let this = v8::undefined(tc_scope).into();
        let args: Vec<v8::Local<v8::Value>> = vec![
            to_v8(tc_scope, batch).expect("Unable to serialize batcher call"),
            to_v8(tc_scope, running).expect("Unable to serialize batcher call"),
        ];

        let result = match cb.call(tc_scope, this, args.as_slice()) {
            Some(result) => result,
            None => return Err(v8_exception(tc_scope)),
        };

        let result: Option<BatcherResult> = from_v8(tc_scope, result).expect(&format!(
            "Unable to deserialize batch for {} due to invalid structure",
            name
        ));
        let next = if idx < batchers_len - 1 {
            Some(idx + 1)
        } else {
            None
        };
        Ok((
            result.unwrap_or(BatcherResult {
                defer: None,
                exec: None,
                completion_map: None,
            }),
            next,
        ))
    }
}

fn v8_exception<'a>(scope: &mut v8::TryCatch<v8::HandleScope>) -> Error {
    let exception = scope.exception().unwrap();
    if is_instance_of_error(scope, exception) {
        let exception: v8::Local<v8::Object> = exception.try_into().unwrap();

        let stack = get_property(scope, exception, "stack");
        let stack: Option<v8::Local<v8::String>> = stack.and_then(|s| s.try_into().ok());
        let stack = stack.map(|s| s.to_rust_string_lossy(scope));
        let err_str = stack.unwrap();
        if err_str.starts_with("Error: ") {
            anyhow!("{}", &err_str[7..])
        } else if err_str.starts_with("TypeError: ") {
            anyhow!("TypeError {}", &err_str[11..])
        } else if err_str.starts_with("SyntaxError: ") {
            anyhow!("SyntaxError {}", &err_str[13..])
        } else if err_str.starts_with("ReferenceError: ") {
            anyhow!("ReferenceError {}", &err_str[16..])
        } else {
            anyhow!("{}", &err_str)
        }
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
