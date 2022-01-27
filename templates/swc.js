Chomp.registerTemplate('swc', function ({ name, targets, deps, env, templateOptions: { configFile = null, noSwcRc = false, sourceMaps = true, config = {}, autoInstall, ...invalid } }, { PATH, CHOMP_EJECT }) {
  if (Object.keys(invalid).length)
    throw new Error(`Invalid swc template option "${Object.keys(invalid)[0]}"`);
  const isWin = PATH.match(/\\|\//)[0] !== '/';
  const defaultConfig = {
    jsc: {
      parser: {
        syntax: 'typescript',
        importAssertions: true,
        topLevelAwait: true,
        importMeta: true,
        privateMethod: true,
        dynamicImport: true
      }/*,
      experimental: {
        keepImportAssertions: true
      }*/ // TODO: reenable when supported
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
  if (noSwcRc) {
    setDefaultConfig(config, defaultConfig);
  }
  return [{
    name,
    targets,
    deps: [...deps, ...noSwcRc || CHOMP_EJECT ? [] : ['.swcrc'], ...CHOMP_EJECT ? [] : ['node_modules/@swc/core', 'node_modules/@swc/cli']],
    env,
    run: `node ./node_modules/@swc/cli/bin/swc.js $DEP -o $TARGET${
        noSwcRc ? ' --no-swcrc' : ''
      }${
        configFile ? ` --config-file=${configFile}` : ''
      }${
        sourceMaps ? ' --source-maps' : ''
      }${
        Object.keys(config).length ? ' ' + Object.keys(config).map(key => `-C ${key}=${config[key]}`).join(' ') : ''
      }`
  }, ...CHOMP_EJECT ? [] : [...noSwcRc ? [] : [{
    target: '.swcrc',
    invalidation: 'not-found',
    display: false,
    run: `
      echo '\n\x1b[93mChomp\x1b[0m: Creating \x1b[1m.swcrc\x1b[0m (set \x1b[1m"no-swc-rc = true"\x1b[0m SWC template option to skip)\n'
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
  }, {
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
        console.log('> Found SWC template usage, select an existing template task to configure, or to create a new template:');
        const num = (await input.choose([
          'New Template',
          ...swcTasks.map(task => task.name || task.target || task.targets[0] || task.run || 'Task ' + chpmpfile.task.indexOf(task)),
        ])).findIndex(x => x);
        if (num === 0) {
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
        dir = dir.replace(/\\\\/g, '/');
        if (dir.startsWith('./')) dir = dir.slice(2);
        if (dir.startsWith('../')) throw new Error('Cannot references paths below the chompfile.');
        if (!dir.endsWith('/')) dir += '/';
        return dir;
      }

      function sanitizeYesNo (result, defaultYesNo) {
        if (result.length === 0) return defaultYesNo;
        switch (result.toLowerCase()) {
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
        task.name = await input.question('Enter a name for the template:', false);
        const inDir = sanitizeDirInput(await input.question('Which folder do you want to build with SWC?'));
        let ext = await input.question('What file extension do you want to build from this folder? [.js]') || '.js';
        if (ext[0] !== '.') ext = '.' + ext;
        task.dep = inDir + '#' + ext;
        task.target = sanitizeDirInput(await input.question('Which folder do you want to output the built JS files to?')) + '#.js';
        task.template = 'swc';
        return task;
      }

      async function cfgTemplate (task) {
        const opts = task['template-options'] = task['template-options'] || TOML.Section({});
        const globalOpts = chompfile['template-options']?.swc || {};
        if (!('auto-install' in opts) && !('auto-install' in globalOpts)) {
          const autoInstall = sanitizeYesNo(await input.question('Automatically install SWC (recommended)? [Yes]', false), true);
          if (autoInstall)
            opts['auto-install'] = true;
        }
        if (!('no-swc-rc' in opts) && !('no-swc-rc' in globalOpts)) {
          const noSwcRc = !sanitizeYesNo(await input.question('Use an .swcrc file (recommended)? [Yes]', false), true);
          if (noSwcRc)
            opts['no-swc-rc'] = true;
        }
        if (opts['no-swc-rc'] || globalOpts['no-swc-rc']) {
          if (!('config-file' in opts) && !('config-file' in globalOpts)) {
            const configFile = await input.question('Custom SWC config file [default: none]:', false);
            if (configFile)
              opts['config-file'] = configFile;
          }
        }
        const cfg = opts['config'] || globalOpts['config'] || {};
        if (!('jsc.parser.syntax' in cfg)) {
          const typescript = sanitizeYesNo(await input.question('Enable SWC TypeScript support? [Yes]', false), true);
          if (!typescript) {
            opts.config = opts.config || TOML.Section({});
            opts.config['jsc.parser.syntax'] = 'ecmascript';
          }
        }
        if (!('jsc.parser.jsx' in cfg)) {
          const jsx = sanitizeYesNo(await input.question('Enable SWC JSX support? [No]', false), false);
          if (jsx) {
            opts.config = opts.config || TOML.Section({});
            opts.config['jsc.parser.jsx'] = true;
          }
        }
        if (cfg['jsc.parser.jsx'] || opts['config']?.['jsc.parser.jsx']) {
          const configFile = await input.question('Custom SWC config file [default: none]:', false);
        }
        if (!('jsc.minify' in cfg)) {
          const minify = sanitizeYesNo(await input.question('Enable SWC minify? [No]', false), false);
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
          const ecmaVersion = choices[(await input.choose(choices)).findIndex(x => x)] || 'es2016';
          opts.config = opts.config || TOML.Section({});
          opts.config['jsc.target'] = ecmaVersion;
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
  }]];
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
