
// TODO: treat core templates as includes
// Chomp.include('https://ga.jspm.io/npm:@chompbuild/template-npm@0.1.1/npm.js');

Chomp.registerTemplate('babel', function ({ name, targets, deps, env, templateOptions: { presets = [], plugins = [], sourceMap = true, babelRc = false, configFile = null, autoInstall, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid babel template option "${Object.keys(invalid)[0]}"`);
  const defaultConfig = {};
  return [{
    name,
    targets,
    deps: [...deps, ...!babelRc || ENV.CHOMP_EJECT ? [] : ['.babelrc'], ...ENV.CHOMP_EJECT ? [] : presets.map(p => `node_modules/${p}`), ...plugins.map(p => `node_modules/${p}`), ...ENV.CHOMP_EJECT ? [] : ['node_modules/@babel/core', 'node_modules/@babel/cli']],
    env,
    run: `babel $DEP -o $TARGET${
        sourceMap ? ' --source-maps' : ''
      }${
        plugins.length ? ` --plugins=${plugins.join(',')}` : ''
      }${
        presets.length ? ` --presets=${presets.join(',')}` : ''
      }${
        !babelRc ? ' --no-babelrc' : ''
      }${
        configFile ? ` --config-file=${configFile.startsWith('./') ? configFile : './' + configFile}` : ''
      }`
  }, ...!babelRc || ENV.CHOMP_EJECT ? [] : [{
    target: '.babelrc',
    display: false,
    invalidation: 'not-found',
    run: `
      echo '\n\x1b[93mChomp\x1b[0m: Creating \x1b[1m.babelrc\x1b[0m (\x1b[1m"babel-rc = true"\x1b[0m Babel template option in use)\n'
      echo '${JSON.stringify(defaultConfig, null, 2)}' > .babelrc
    `
  }], ...ENV.CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      packages: [...presets.map(p => p.startsWith('@babel/') ? p + '@7' : p), ...plugins.map(p => p.startsWith('@babel/') ? p + '@7' : p), '@babel/core@7', '@babel/cli@7'],
      dev: true,
      autoInstall
    }
  }]];
});

Chomp.registerBatcher('babel', function (batch, _running) {
  const run_completions = {};
  let existingBabelRcInit = null;
  for (const { id, run, engine } of batch) {
    if (engine !== 'cmd' || !run.trimLeft().startsWith('echo ')) continue;
    if (run.indexOf('Creating \x1b[1m.babelrc\x1b[0m') !== -1) {
      if (existingBabelRcInit !== null) {
        run_completions[id] = existingBabelRcInit;  
      }
      else {
        existingBabelRcInit = id;
      }
      continue;
    }
  }
  return [[], [], run_completions];
});
Chomp.registerTemplate('cargo', function ({ deps, env, templateOptions: { bin, install, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid cargo template option "${Object.keys(invalid)[0]}"`);
  const sep = PATH.match(/\\|\//)[0];
  return ENV.CHOMP_EJECT ? [] : [{
    name: `cargo:${bin}`,
    targets: [PATH.split(';').find(p => p.endsWith(`.cargo${sep}bin`)) + sep + bin + (sep === '/' ? '' : '.exe')],
    invalidation: 'not-found',
    display: false,
    deps,
    env,
    run: `cargo install ${install}`
  }];
});
Chomp.registerTemplate('jspm', function ({ name, targets, deps, env, templateOptions: {
  autoInstall,
  env: generatorEnv = ['browser', 'production', 'module'],
  preload,
  integrity,
  whitespace,
  esModuleShims,
  ...generateOpts
} }) {
  const mainTarget = targets.find(target => target.includes('#')) || targets[0];
  const isImportMapTarget = mainTarget && mainTarget.endsWith('.importmap');
  const { resolutions } = generateOpts;
  const noHtmlOpts = preload === undefined && integrity === undefined && whitespace === undefined && esModuleShims === undefined;
  return [{
    name,
    targets,
    invalidation: 'always',
    deps: [...deps, ...ENV.CHOMP_EJECT ? [] : ['node_modules/@jspm/generator', 'node_modules/mkdirp']],
    env,
    engine: 'node',
    run: `    import { Generator } from '@jspm/generator';
    import { readFile, writeFile } from 'fs/promises';
    import { pathToFileURL } from 'url';
    import mkdirp from 'mkdirp';
    import { dirname } from 'path';

    const generator = new Generator({
      mapUrl: ${isImportMapTarget ? 'import.meta.url' : 'pathToFileURL(process.env.TARGET)'}${
        resolutions && !isImportMapTarget && Object.values(resolutions).some(v => v.startsWith('./') || v.startsWith('../')) ? ',\n      baseUrl: new URL(\'.\', import.meta.url)' : ''
      },\n      env: ${JSON.stringify(generatorEnv).replace(/","/g, '", "')}${
        Object.keys(generateOpts).length ? ',\n      ' + JSON.stringify(generateOpts, null, 2).slice(4, -2).replace(/\n/g, `\n    `) : ''
      }
    });
${isImportMapTarget ? `
    await Promise.all(process.env.DEPS.split(',')${ENV.CHOMP_EJECT ? '' : '.filter(dep => dep !== "node_modules/@jspm/generator" && dep !== "node_modules/mkdirp")'}.map(dep => generator.traceInstall('./' + dep)));

    mkdirp.sync(dirname(process.env.TARGET));
    await writeFile(process.env.TARGET, JSON.stringify(generator.getMap(), null, 2));`
: `
    const htmlSource = await readFile(process.env.DEP, 'utf-8');

    mkdirp.sync(dirname(process.env.TARGET));
    await writeFile(process.env.TARGET, await generator.htmlGenerate(htmlSource, {
      htmlUrl: pathToFileURL(process.env.TARGET)${noHtmlOpts ? '' : ',      ' + JSON.stringify({ preload, integrity, whitespace, esModuleShims })}
    }));`}
`
  }, ...ENV.CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['@jspm/generator', 'mkdirp'],
      dev: true
    }
  }]];
});
Chomp.registerTemplate('npm', function ({ name, deps, env, templateOptions: { packages, dev, packageManager = 'npm', autoInstall, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid npm template option "${Object.keys(invalid)[0]}"`);
  if (!packages)
    throw new Error('npm template requires the "packages" option to be a list of packages to install.');
  return ENV.CHOMP_EJECT ? [] : autoInstall ? [{
    name,
    deps: [...deps, ...packages.map(pkg => {
      const versionIndex = pkg.indexOf('@', 1);
      return `node_modules/${versionIndex === -1 ? pkg : pkg.slice(0, versionIndex)}`;
    })],
    serial: true
  }, ...packages.map(pkg => {
    const versionIndex = pkg.indexOf('@', 1);
    return {
      target: `node_modules/${versionIndex === -1 ? pkg : pkg.slice(0, versionIndex)}`,
      invalidation: 'not-found',
      display: false,
      deps: ['npm:init'],
      env,
      run: `${packageManager} install ${packages.join(' ')}${dev ? ' -D' : ''}`
    };
  }), {
    name: 'npm:init',
    target: 'package.json',
    invalidation: 'not-found',
    display: false,
    env,
    run: `${packageManager} init -y`
  }] : [{
    name,
    env,
    invalidation: 'not-found',
    display: false,
    targets: packages.map(pkg => {
      const versionIndex = pkg.indexOf('@', 1);
      return `node_modules/${versionIndex === -1 ? pkg : pkg.slice(0, versionIndex)}`;
    }),
    run: `echo "\n\x1b[93mChomp\x1b[0m: Some packages are missing. Please run \x1b[1m${packageManager} install ${packages.join(' ')}${dev ? ' -D' : ''}\x1b[0m\n"`
  }];
});

// Batcher for npm executions handles the following:
// 1. Ensuring only one npm operation runs at a time
// 2. If two npm init operations are batched, only one is run. If npm init
//    is already running, ties additional invocations to the existing one.
// 3. When multiple npm install operations are running at the same time,
//    combine them into a single install operation.
Chomp.registerBatcher('npm', function (batch, running) {
  const queued = [], run_completions = {};
  let batchInstall = null;
  for (const { id, run, engine, env } of batch) {
    if (engine !== 'cmd' || !run.startsWith('npm ')) continue;
    const args = run.slice(4).split(' ');
    if (args[0] === 'init' && args[1] === '-y' && args.length === 2) {
      const existingNpm = running.find(({ run }) => run.startsWith('npm '));
      if (existingNpm) {
        run_completions[id] = existingNpm.id;
        continue;
      }
    }
    if (args[0] === 'install') {
      const install = parseInstall(args.slice(1));
      if (!install) return;
      if (running.find(({ run }) => run.startsWith('npm ')) ||
          batchInstall && batchInstall.isDev !== install.isDev) {
        queued.push(id);
        continue;
      }
      if (!batchInstall) {
        batchInstall = { isDev: install.isDev, env, engine, ids: [id], packages: install.packages };
      }
      else {
        for (const key of Object.keys(env)) {
          if (!Object.hasOwnProperty.call(batchInstall.env, key))
            batchInstall.env[key] = env[key];
        }
        batchInstall.ids.push(id);
        for (const pkg of install.packages) {
          if (!batchInstall.packages.includes(pkg))
            batchInstall.packages.push(pkg);
        }
      }
    }
  }
  const run = batchInstall ? [{
    run: `npm install${batchInstall.isDev ? ' -D' : ''} ${batchInstall.packages.join(' ')}`,
    env: batchInstall.env,
    engine: batchInstall.engine,
    ids: batchInstall.ids,
  }] : [];
  return [queued, run, run_completions];

  function parseInstall (args) {
    const packages = args.filter(arg => !arg.startsWith('-') && arg.indexOf('"') === -1 && arg.indexOf("'") === -1);
    const flags = args.filter(arg => arg.startsWith('-'));
    if (flags.length > 1) return;
    if (flags.length === 1 && flags[0] !== '-D') return;
    if (packages.length + flags.length !== args.length) return;
    const isDev = flags.length === 1;
    return { packages, isDev };
  }
});
Chomp.registerTemplate('prettier', function ({ name, targets, deps, env, templateOptions: { files = '.', check = false, write = true, config = null, noErrorOnUnmatchedPattern = false, autoInstall, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid prettier template option "${Object.keys(invalid)[0]}"`);
  return [{
    name,
    targets,
    deps: [...deps, ...ENV.CHOMP_EJECT ? [] : ['node_modules/prettier']],
    invalidation: 'always',
    env,
    run: `prettier ${files} ${
        check ? ' --check' : ''
      }${
        write ? ' --write' : ''
      }${
        config ? ` --config ${config}` : ''
      }${
        noErrorOnUnmatchedPattern ? ' --no-error-on-unmatched-pattern' : ''
      }`
  }, ...ENV.CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['prettier'],
      dev: true
    }
  }]];
});
Chomp.registerTemplate('svelte', function ({ name, targets, deps, env, templateOptions: { svelteConfig = null, autoInstall, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid svelte template option "${Object.keys(invalid)[0]}"`);
  return [{
    name,
    targets,
    deps: [...deps, ...ENV.CHOMP_EJECT ? [] : ['node_modules/svelte', 'node_modules/mkdirp']],
    env,
    engine: 'node',
    run: `    import { readFile, writeFile } from 'fs/promises';
      import { compile } from 'svelte/compiler';
      import mkdirp from 'mkdirp';
      import { dirname } from 'path';

      let config;
      ${svelteConfig ? `
        config = await import(${svelteConfig === true ? '"./svelte.config.js"' : svelteConfig});
      ` : `
        config = {
          css: false
        };
      `}
      config.filename = process.env.DEP;

      const source = await readFile(process.env.DEP, 'utf-8');
      const result = compile(source, config);

      mkdirp.sync(dirname(process.env.TARGET));
      const cssFile = process.env.TARGET.replace(/\\.js$/, ".css");
      await Promise.all[
        writeFile(process.env.TARGET, result.js.code),
        writeFile(process.env.TARGET + ".map", JSON.stringify(result.js.map)),
        writeFile(cssFile, result.css.code),
        writeFile(cssFile + ".map", JSON.stringify(result.css.map))
      ];
    `
  }, ...ENV.CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['svelte@3', 'mkdirp'],
      dev: true
    }
  }]];
});
Chomp.registerTemplate('swc', function ({ name, targets, deps, env, templateOptions: { configFile = null, swcRc = false, sourceMaps = true, config = {}, autoInstall, ...invalid } }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid swc template option "${Object.keys(invalid)[0]}"`);
  const isWin = ENV.PATH.match(/\\|\//)[0] !== '/';
  const defaultConfig = {
    jsc: {
      parser: {
        syntax: 'typescript',
        importAssertions: true,
        topLevelAwait: true,
        importMeta: true,
        privateMethod: true,
        dynamicImport: true
      },
      target: 'es2016',
      experimental: {
        keepImportAssertions: true
      }
    }
  };
  function setDefaultConfig (config, defaultConfig, base = '') {
    for (const prop of Object.keys(defaultConfig)) {
      const val = defaultConfig[prop];
      if (typeof val === 'object') {
        setDefaultConfig(config, defaultConfig[prop], base + prop + '.');
      }
      else if (!((base + prop) in config)) {
        config[base + prop] = defaultConfig[prop];
      }
    }
  }
  if (!swcRc) {
    setDefaultConfig(config, defaultConfig);
  }
  return [{
    name,
    targets,
    deps: [...deps, ...!swcRc || env.CHOMP_EJECT ? [] : ['.swcrc'], ...env.CHOMP_EJECT ? [] : ['node_modules/@swc/core', 'node_modules/@swc/cli']],
    env,
    run: `node ./node_modules/@swc/cli/bin/swc.js $DEP -o $TARGET${
        !swcRc ? ' --no-swcrc' : ''
      }${
        configFile ? ` --config-file=${configFile}` : ''
      }${
        sourceMaps ? ' --source-maps' : ''
      }${
        Object.keys(config).length ? ' ' + Object.keys(config).map(key => `-C ${key}=${config[key]}`).join(' ') : ''
      }`
  }, ...env.CHOMP_EJECT ? [] : [...swcRc ? [] : [{
    target: '.swcrc',
    invalidation: 'not-found',
    display: false,
    run: `
      echo '\n\x1b[93mChomp\x1b[0m: Creating \x1b[1m.swcrc\x1b[0m (\x1b[1m"swc-rc = true"\x1b[0m SWC template option in use)\n'
      ${isWin // SWC does not like a BOM... Powershell hacks...
        ? `$encoder = new-object System.Text.UTF8Encoding ; Set-Content -Value $encoder.Getbytes('${JSON.stringify(defaultConfig, null, 2)}') -Encoding Byte -Path $TARGET`
        : `echo '${JSON.stringify(defaultConfig)}' > $TARGET`
      }
    `
  }], {
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['@swc/core@1', '@swc/cli@0.1'],
      dev: true
    }
  }]];
});

Chomp.registerTask({
  name: 'swc:init',
  engine: 'deno',
  run: `
    import TOML from 'https://jspm.dev/@ltd/j-toml@1';
    import InputLoop from 'https://deno.land/x/input@2.0.3/index.ts';

    const chompfile = TOML.parse(new TextDecoder('utf-8').decode(Deno.readFileSync('chompfile.toml', 'utf-8')));

    const swcTasks = (chompfile.task || []).filter(task => task.template === 'swc');

    console.log('SWC Chompfile Template Configuration Utility');

    const input = new InputLoop();

    let task;
    if (swcTasks.length) {
      console.log('Found SWC template usage, select an existing template task to configure, or to create a new template:');
      const num = (await input.choose([
        'New Template',
        ...swcTasks.map(task => task.name || task.target || task.targets?.[0] || task.run || 'Task ' + chompfile.task.indexOf(task)),
      ])).findIndex(x => x);
      if (num === 0 || num === -1) {
        task = await newTemplate();
      }
      else {
        task = swcTasks[num - 1];
      }
    }
    else {
      console.log("No SWC template found, creating a new template...");
      task = await newTemplate();
    }
    await cfgTemplate(task);

    function sanitizeDirInput (dir) {
      dir = dir.replace(/\\\\/g, '/').trim();
      if (dir.startsWith('./')) dir = dir.slice(2);
      if (dir.startsWith('../')) throw new Error('Cannot references paths below the chompfile.');
      if (!dir.endsWith('/')) dir += '/';
      return dir;
    }

    function sanitizeYesNo (result, defaultYesNo) {
      if (result.length === 0) return defaultYesNo;
      switch (result.toLowerCase().trim()) {
        case 'y':
        case 'yes':
          return true;
        case 'n':
        case 'no':
          return false;
      }
      throw new Error('Invalid response.');
    }

    async function newTemplate () {
      const task = {};
      const name = (await input.question('Enter a name for the template (optional): ', false)).trim();
      if (name) {
        if (name.indexOf(' ') !== -1) throw new Error('Task name cannot have spaces');
        if (chompfile.task.some(t => t.name === name)) throw new Error('A task "' + task.name + '" already exists.');
        task.name = name;
      }
      const inDir = sanitizeDirInput(await input.question('Which folder do you want to build with SWC? [src] ', false) || 'src');
      let ext = await input.question('What file extension do you want to build from this folder? [.js] ', false) || '.js';
      if (ext[0] !== '.') ext = '.' + ext;
      task.dep = inDir + '#' + ext.trim();
      task.target = sanitizeDirInput(await input.question('Which folder do you want to output the built JS files to? [lib] ', false) || 'lib') + '#.js';
      task.template = 'swc';
      (chompfile.task = chompfile.task || []).push(task);
      return task;
    }

    async function cfgTemplate (task) {
      const opts = task['template-options'] = task['template-options'] || TOML.Section({});
      const globalOpts = chompfile['template-options']?.swc || {};
      if (!('auto-install' in opts) && !('auto-install' in globalOpts)) {
        const autoInstall = sanitizeYesNo(await input.question('Automatically install SWC (recommended)? [Yes] ', false), true);
        if (autoInstall)
          opts['auto-install'] = true;
      }
      if (!('swc-rc' in opts) && !('swc-rc' in globalOpts)) {
        const swcRc = sanitizeYesNo(await input.question('Use an .swcrc file? [No] ', false), false);
        if (swcRc)
          opts['swc-rc'] = true;
      }
      if (!opts['swc-rc'] && !globalOpts['swc-rc']) {
        if (!('config-file' in opts) && !('config-file' in globalOpts)) {
          const configFile = await input.question('Custom SWC config file [default: none]: ', false);
          if (configFile)
            opts['config-file'] = configFile;
        }
        const cfg = opts['config'] || globalOpts['config'] || {};
        let typescript = cfg['jsc.parser.syntax'] === 'typescript';
        if (!('jsc.parser.syntax' in cfg)) {
          typescript = sanitizeYesNo(await input.question('Enable SWC TypeScript support? [Yes] ', false), true);
          if (!typescript) {
            opts.config = opts.config || TOML.Section({});
            opts.config['jsc.parser.syntax'] = 'ecmascript';
          }
        }
        if (!('jsc.parser.jsx' in cfg)) {
          const jsx = sanitizeYesNo(await input.question('Enable SWC JSX support? [No] ', false), false);
          if (jsx) {
	    if (typescript) throw new Error('SWC doesnt currently support TypeScript + JSX via inline config. Use an .swcrc file instead.');
            opts.config = opts.config || TOML.Section({});
            opts.config['jsc.parser.jsx'] = true;
          }
        }
        if (cfg['jsc.parser.jsx'] || opts['config']?.['jsc.parser.jsx']) {
          const configFile = await input.question('Custom SWC config file [default: none]: ', false);
        }
        if (!('jsc.minify' in cfg)) {
          const minify = sanitizeYesNo(await input.question('Enable SWC minify? [No] ', false), false);
          if (minify) {
            opts.config = opts.config || TOML.Section({});
            opts.config['jsc.minify'] = true;
          }
        }
        if (!('jsc.target' in cfg)) {
          const choices = [
            'es2015',
            'es2016',
            'es2017',
            'es2018',
            'es2019',
            'es2020',
            'es2021',
            'es2022'
          ];
          console.log('Select SWC Target [es2016]');
          const ecmaVersion = choices[(await input.choose(choices)).findIndex(x => x)];
          if (ecmaVersion) {
            opts.config = opts.config || TOML.Section({});
            opts.config['jsc.target'] = ecmaVersion;
          }
        }
      }
    }

    // Try to match formatting with "chomp -F" Rust serde formatting
    Deno.writeFileSync('chompfile.toml', new TextEncoder().encode(TOML.stringify(chompfile, {
      newline: '\\n',
      newlineAround: 'section',
      indent: '    '
    }).slice(1)));

    console.log('chompfile.toml updated successfully.');
  `
});

// Batcher to ensure swcrc log only appears once
Chomp.registerBatcher('swc', function (batch, running) {
  const run_completions = {};
  let existingSwcRcInit = null;
  for (const { id, run, engine, env } of batch) {
    if (engine !== 'cmd' || !run.trimLeft().startsWith('echo ')) continue;
    if (run.indexOf('Creating \x1b[1m.swcrc\x1b[0m') !== -1) {
      if (existingSwcRcInit !== null) {
        run_completions[id] = existingSwcRcInit;
      }
      else {
        existingSwcRcInit = id;
      }
      continue;
    }
  }
  return [[], [], run_completions];
});
