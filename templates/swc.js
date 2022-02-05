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
