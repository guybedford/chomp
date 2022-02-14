Chomp.registerTemplate('assert', function (task) {
  if (task.templateOptions.expectDep)
    env['EXPECT_DEP'] = task.templateOptions.expectDep;
  if (task.templateOptions.expectTarget)
    env['EXPECT_TARGET'] = task.templateOptions.expectTarget;
  return [{
    dep: '&next',
    engine: 'node',
    run: `
      import { strictEqual } from 'assert';
      import { readFileSync } from 'fs';

      function rnlb (source) {
        if (source.startsWith('\\ufeff'))
          source = source.slice(1);
        if (source.endsWith('\\r\\n'))
          source = source.slice(0, -2);
        else if (source.endsWith('\\n'))
          source = source.slice(0, -1);
        return source;
      }

      let asserted = false;

      if (process.env.EXPECT_DEP) {
        strictEqual(rnlb(readFileSync(process.env.DEP, 'utf8')), rnlb(process.env.EXPECT_DEP));
        asserted = true;
      }

      if (process.env.EXPECT_TARGET) {
        strictEqual(rnlb(readFileSync(process.env.TARGET, 'utf8')), rnlb(process.env.EXPECT_TARGET));
        asserted = true;
      }

      if (!asserted) {
        throw new Error('Chomp assert template did not assert anything! There must be an "expect-dep" or "expect-target" check.');
      }
    `
  }, {
    ...task,
    deps: [...task.deps]
  }];
});
