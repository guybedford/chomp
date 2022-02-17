Chomp.registerTemplate('assert', function (task) {
  let env = {};
  if (task.templateOptions.expectEquals) {
    env['EXPECT_EQUALS'] = task.templateOptions.expectEquals;
    if (task.targets.length === 0)
      throw new Error('Assertion tests must have a target to assert.');
    if (task.targets.some(target => target.indexOf('#') !== -1))
      throw new Error('Assertion tests do not support interpolates.');
    env['ASSERT_TARGET'] = task.targets[0];
  }
  if (!task.name)
    throw new Error('Assertion tests must be named.');
  const name = task.name;
  delete task.name;
  return [{
    name,
    dep: '&next',
    engine: 'node',
    env,
    display: 'status-only',
    run: `
      import { strictEqual } from 'assert';
      import { readFileSync } from 'fs';

      function rnlb (source) {
        source = source.replace(/\\r\\n/g, '\\n');
        if (source.startsWith('\\ufeff'))
          source = source.slice(1);
        if (source.endsWith('\\n'))
          source = source.slice(0, -1);
        return source;
      }

      let asserted = false;
      if (process.env.EXPECT_EQUALS) {
        strictEqual(rnlb(readFileSync(process.env.ASSERT_TARGET, 'utf8')), rnlb(process.env.EXPECT_EQUALS));
        asserted = true;
      }
      if (!asserted) {
        throw new Error('Chomp assert template did not assert anything! There must be an "expect-dep" or "expect-target" check.');
      }
    `
  }, task];
});
