# Extensions

## Overview

Executions are loaded through the embedded V8 environment in Chomp, which includes a very simple `console.log` implementation and basic error handling, an `ENV` global detailed below and a `Chomp` global detailed below.

Extensions must be declared in the active `chompfile.toml` in use via the `extensions` list, and can be loaded from any path or URL.

URL extensions are fetched from the network and cached indefinitely in the global Chomp cache folder.

All executions are immediately invoked during the initialization phase, and all registrations must be made during this phase. Any
hook registrations via the `Chomp` global made after this initialization phase will throw an error.

Registrations made by Chomp extensions can then hook into phases of the Chomp task running lifecycle including the process of task list population, template expansion and batching of tasks.

### Publishing Extensions

When developing extensions, it is recommended to load them by relative file paths:

chompfile.toml
```toml
version = 0.1

extensions = ['./local-extension.js']
```

When sharing the extension between projects, hosting it on any remote URL is supported by Chomp.

Note that remote URLs are cached indefinitely regardless of cache headers for performance so it is recommended to include the version in the URL. `chomp --cache-clear` can be used to clear this remote cache.

If publishing to npm, templates will be available on any npm CDN like `unpkg.com` or `ga.jspm.io`.

If publishing to JSPM, set the `package.json` property `"type": "script"` to inform JSPM the `.js` files are scripts and not modules to avoid incorrect processing.

## API

JavaScript extensions register hooks via the `Chomp` global scripting interface.

TypeScript typing is not currently available for the `Chomp` global. PRs to provide this typing integration would be welcome.

### ENV

The `ENV` JS global is available in extensions, and contains all environment variables as a dictionary.

The following Chomp-specific environment variables are also defined:

* `ENV.CHOMP_EJECT`: When `--eject` is passed for template injection this is set to `"1"`.
* `ENV.CHOMP_POOL_SIZE`: Set to the maximum number of jobs for Chomp, which is usually the CPU count, or the value passed to the `-j` flag when running Chomp.

### Core Templates

Some [Chomp templates](https://github.com/guybedford/chomp-templates) are provided for the JS ecosystem, and PRs to this repo are very welcome.

These templates can be loaded via the `chomp:[name]` extension names.

By default these templates are loaded from the JSPM CDN at `https://ga.jspm.io/npm:@chompbuild/templates@x.y.z/[name].js`.

This path can be overridden to an alternative remote URL or local path by setting the `CHOMP_CORE` environment variable.

### Chomp.addExtension(extension: string)

Extensions may load other extensions from any path or URL. Relative URLs are supported for loading extensions relative to the current extension location. Examples:

```js
Chomp.addExtension('https://site.com/extension.js');
Chomp.addExtension('./local.js');
```

Extensions are resolved to absolute URLs internally so that a given extension can only be loaded once even if `addExtension` is
called repeatedly on the same extension script.

### Chomp.registerTask(task: ChompTask)

Arbitrary tasks may be added as if they were defined in the users Chompfile. This is useful for common tasks
that are independent of the exact project, such as initialization, workflow and bootstrapping tasks.

`ChompTask` is the same interface as in the TOML definition, except that base-level kebab-case properties are
instead provided as camelCase.

Note that extension-registered tasks are not output when running template ejection via `chomp --eject`.

#### Example: Configuration Initialization Task

An example of an initialization task is to create a configuration file if it does not exist:

```js
Chomp.registerTask({
  name: 'config:init',
  engine: 'node',
  target: 'my.config.json',
  run: `
    import { writeFileSync } from 'fs';

    const defaultConfig = {
      some: 'config'
    };

    // (this task only never runs when my.config.json does not exist)
    writeFileSync(process.env.TARGET, JSON.stringify(defaultConfig, null, 2));

    console.log(\`\${process.env.TARGET} initialized successfully.\`);
  `
});
```

### Chomp.registerTemplate(name: string, template: (task: ChompTask) => ChompTask[])

Registers a template function to a template name. In the case of multiple registration, the last registered template function for a given template name will apply, permitting overrides.

Template task expansion happens early during initialization, and is independent of user options. All template tasks are expanded into
untemplated tasks internally until the final flat non-templated task list is found, which is used as the task list for the runner.

Tasks with a `template` field will call the associated registered template function with the task as the first argument to the template function. The template function can then return an array of tasks to register for the current run (whether they execute is still as defined by the task graph). Templates may return tasks that in turn use templates, which are then expanded recursively.

When `--eject` is used, this same expanded template list is saved back to the `chompfile.toml` itself to switch to an untemplated
configuration form. The `ENV.CHOMP_EJECT` global variable can be used to branch behaviour during ejection to provide a more user-suitable output where appropriate.

For template usage options validation, normal JS errors thrown will be reported with their message to the user. Template options
should be validated this way.

#### Example: Runner Template

An example of a simple run template to abstract the execution details of a task:

```toml
version = 0.1

[[task]]
name = 'template-example'
template = 'echo'

[task.template-options]
message = 'chomp chomp'
```

With execution `chomp template-example` writing `chomp chomp` to the console.

```js
Chomp.registerTemplate('echo', function (task) {
  if (typeof task.templateOptions.message !== 'string')
    throw new Error('Echo template expects a string message in template-options.');
  if (task.run || task.engine)
    throw new Error('Echo template does not expect a run or engine field.');
  return [{
    // task "name" and "deps" need to be passed through manually
    // and similarly for tasks that define targets
    name: task.name,
    deps: task.deps,
    run: `echo ${JSON.stringify(task.templateOptions.message)}`
  }];
});
```

Templates get a lot more useful when they use the Deno or Node engines, as they can then
fully encapsulate custom wrapper code for arbitrary computations from ecosystem libraries
that do not have CLIs.

### Chomp.registerBatcher(name: string, batcher: (batch: CmdOp[], running: BatchCmd[]) => BatcherResult | undefined)

#### Overview

Batchers act as reducers of task executions into system execution calls. They allow for custom queueing and coalescing of task runs.

For example, consider a task that performs an `npm install` - only one `npm install` operation must ever run at a time, if a previous install is running the task should wait for it to finish executing first (queuing). Furthermore, if two npm installs (`npm install a` and `npm install b` say) are queued at the same time they can be combined together into a single npm install call: `npm install a b` (coalescing).

This is the primary use case for batchers - combining together task executions into singular executions where that will save time.

_Batching is a complex topic, and is more about the exact use case solutions at hand. In most cases extensions needn't worry about batching until they really need to carefully optimize and control execution invocations for performance._

#### Lifecycle

Task executions are collected as a `CmdOp` list, with batching run periodically against this list. Batchers then combine and queue executions as necessary.

Under the batching model, the lifecycle of an execution includes the following steps:

1. Task is batched as a `CmdOp` representing the execution of the task (the `run` and `engine` pair). This forms the batch command queue, `batch`, which is a fixed list for a given batching operation.
2. Every 5 milliseconds, if there are batched commands, the batcher phase is initiated on the `batch` list, where all registered batchers are each passed the `batch` queue to process it in order. They are also passed the list of running executions `running` as the second argument.
3. As each `CmdOp` in the `batch` is processed by a batcher, by being assigned by the `BatcherResult` of the batcher, it is removed from the `batch` list so the next batcher will not see it. The final _default batcher_ will just naively run the execution with simple CPU-based pooling.

#### CmdOp

The queued commands of `CmdOp` are defined as:

```typescript
interface CmdOp {
  id: number,
  run: string,
  engine: 'deno' | 'node' | 'cmd',
  name?: string,
  cwd?: string,
  env: Record<string, string>,
}
```

The `id` of the task operation is importantly used to key the batching process. Task executions are primarily defined by their `run` and `engine` pair.

#### BatchCmd

Batched execution commands have an almost identical interface to `CmdOp`, except with a list of `ids` of the `CmdOp` ids whose completion is fulfilled by this execution.

```typescript
interface BatchCmd {
  ids: number[],
  run: string,
  engine: 'deno' | 'node' | 'cmd',
  cwd?: string,
  env: BTreeMap<string, string>,
}
```

Each `BatchCmd` real spawned execution thus corresponds to one or more `CmdOp` execution, as the reduction output of batching.

#### BatcherResult

`BatcherResult`, the return value of the batching function, forms the execution combining and queuing operation of the batcher. It has three optional return properties - `queue`, `exec` and `completionMap`. `queue` is a list of tasks to defer for the next batch queue allowing the ability to delay their execution. `exec` is the list of `BatchCmd` executions to immediately invoke. And `completionMap`, less commonly used, allows associating the completion of one of the operations in the batch to the completion of a currently running task in the already-`running` list.

```typescript
interface BatcherResult {
  queue?: number[],
  exec?: BatchCmd[],
  completionMap?: Record<number, number>,
}
```

As soon as any batcher assigns the `id` of `CmdOp` via one of these properties, that task command is considered assigned, and removed from the batch list for the next batcher. At the end of calling all batchers, any remaining task commands in the batch list are just batched by the default batcher.

#### Example: Default Batcher

The code for the default batcher is a good example of how simple batching can work. It does not combine executions so creates one batch execution for each task execution, while respecting the job limit reflected via `ENV.CHOMP_POOL_SIZE` which ensures the CPUs is utilized efficiently:

```js
// Chomp's default batcher, without any batcher extensions:
const POOL_SIZE = Number(ENV.CHOMP_POOL_SIZE);

Chomp.registerBatcher('defaultBatcher', function (batch, running) {
  // If we are already running the maximum number of jobs, defer the whole batch
  if (running.length >= POOL_SIZE)
    return { defer: batch.map(({ id }) => id) };
  
  return {
    // Create a single execution for every item in the batch, up to the pool size
    exec: batch.slice(0, POOL_SIZE - running.length).map(({ id, run, engine, name, cwd, env }) => ({
      ids: [id],
      run,
      engine,
      cwd,
      env
    })),
    // Once we hit the pool size limit, defer the remaining batched executions
    defer: batch.slice(POOL_SIZE - running.length).map(({ id }) => id)
  };
});
```

Because batchers run one after another, having this exact above default batcher run last means it will take care of pooling so most batchers don't need to worry so much about it. Instead most batchers just focus on the specific executions they are interested in to run, queue and combine those executions they care about specifically, while within the pool limit.

Thus, most batchers are of the form:

```js
const POOL_SIZE = Number(ENV.CHOMP_POOL_SIZE);
Chomp.registerBatcher('my-batcher', function (batch, running) {
  if (running.length >= POOL_SIZE) return;

  const exec = [];
  for (const item of batch) {
    // ignore anything not intresting to this batcher, or if we have hit the pool limit
    if (!is_interesting(item) || exec.length + running.length >= POOL_SIZE) continue;
    
    // push the batched execution we're interested in,
    // usually matching it up with another to combine their executions
    exec.push({ ...item, ids: [item.id] });
  }

  return { exec };
});
```

#### Running List and Completion Map

All commands that have already been spawned and have not returned a final status code and terminated their running OS process
are provided in the `running` list as the second argument to the batcher.

In the `running` list, each `BatchCmd` will also have an associated `id` corresponding to the batch id, which is distinct from the `CmdOp` id.

The completion map is a map from `CmdOp` id to `BatchCmd` id, which allows associating the current batch execution fulfillment with the completion of a currently executing task. This is useful for eg a generic `npm install` operation, where if an `npm install` is already running we should simply attach to that instead of queueing another `npm install`. Effectively a mapping forming a late adding to the `ids` list of that previously batched command. Because multiple `CmdOp`s can map to the same `BatchCmd` in this map structure, it supports the same type of many-to-one completion attachment as the `ids` list, just for already-running tasks as opposed to as part of the baching reduction to begin with - the difference being already running tasks cannot be altered or stopped as they have already started.

#### Combining Tasks

The coalescing of batch tasks implies reparsing the `run` or `env` vars of `CmdOp` task executions to collate them into a single `run` and `env` on a `BatchCmd` return of the `exec` property of the `BatcherResult`. It was a choice between this model, or modelling the data structure of the command calls more abstractly first, which seemed unnecessary overhead when execution parsing can suffice and with the flexibility of the JS language implementation the edge cases can generally be well handled.

See the [Chomp templates](https://guybedford/chomp-templates) repo for further understanding through the direct examples of these hooks in use. The `npm.js` batcher demonstrates `npm install` batching and the `swc.js` batcher demonstrates compilation batching.
