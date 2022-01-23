Chomp.registerTemplate('jspm', function ({ name, targets, deps, env, templateOptions: {
  autoInstall,
  env: generatorEnv = ['browser', 'production', 'module'],
  preload,
  integrity,
  whitespace,
  esModuleShims,
  ...generateOpts
} }, { CHOMP_EJECT }) {
  const mainTarget = targets.find(target => target.includes('#')) || targets[0];
  const isImportMapTarget = mainTarget && mainTarget.endsWith('.importmap');
  const { resolutions } = generateOpts;
  const noHtmlOpts = preload === undefined && integrity === undefined && whitespace === undefined && esModuleShims === undefined;
  return [{
    name,
    targets,
    invalidation: 'always',
    deps: [...deps, ...CHOMP_EJECT ? [] : ['node_modules/@jspm/generator', 'node_modules/mkdirp']],
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
    await Promise.all(process.env.DEPS.split(',')${CHOMP_EJECT ? '' : '.filter(dep => dep !== "node_modules/@jspm/generator" && dep !== "node_modules/mkdirp")'}.map(dep => generator.traceInstall('./' + dep)));

    mkdirp.sync(dirname(process.env.TARGET));
    await writeFile(process.env.TARGET, JSON.stringify(generator.getMap(), null, 2));`
: `
    const htmlSource = await readFile(process.env.DEP, 'utf-8');

    mkdirp.sync(dirname(process.env.TARGET));
    await writeFile(process.env.TARGET, await generator.htmlGenerate(htmlSource, {
      htmlUrl: pathToFileURL(process.env.TARGET)${noHtmlOpts ? '' : ',      ' + JSON.stringify({ preload, integrity, whitespace, esModuleShims })}
    }));`}
`
  }, ...CHOMP_EJECT ? [] : [{
    template: 'npm',
    templateOptions: {
      autoInstall,
      packages: ['@jspm/generator', 'mkdirp'],
      dev: true
    }
  }]];
});
