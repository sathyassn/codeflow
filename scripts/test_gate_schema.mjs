import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// The portal's locked toolchain already includes Ajv. This structural check
// runs after its locked install, without installing another test dependency.
const require = createRequire(new URL('../docs-portal/package.json', import.meta.url));
const Ajv2020 = require('ajv/dist/2020').default;
const root = resolve(process.env.CODEFLOW_GATE_CONTRACT_ROOT ?? fileURLToPath(new URL('..', import.meta.url)));
const paths = ['assets/base/testing/test-config.schema.json', '.codeflow/test-config.schema.json'];
const oldConfig = { schema_version: '1.0', execution: { parallel: false }, targets: [{ name: 'tests', runner: 'custom', modes: { full: { command: 'true' } } }] };
const newConfig = structuredClone(oldConfig);
Object.assign(newConfig.execution, { max_parallel: 2, run_everything: ['crates/**'] });
Object.assign(newConfig.targets[0], { requires: ['build'], outputs: ['result.xml'], narrow: ['docs/**'], exclusive: true });

test('both schemas accept legacy and prerequisite fields and reject unknown fields', () => {
  const schemas = paths.map(path => readFileSync(resolve(root, path), 'utf8'));
  for (const schema of schemas) {
    const validate = new Ajv2020({ strict: false }).compile(JSON.parse(schema));
    for (const config of [oldConfig, newConfig, JSON.parse(readFileSync(resolve(root, '.codeflow/test-config.json'), 'utf8'))]) assert.ok(validate(config), JSON.stringify(validate.errors));
    for (const where of ['root', 'execution', 'target']) {
      const config = structuredClone(newConfig);
      (where === 'root' ? config : where === 'execution' ? config.execution : config.targets[0]).unknown = true;
      assert.equal(validate(config), false, where);
    }
    const config = structuredClone(newConfig);
    config.execution.max_parallel = 0;
    assert.equal(validate(config), false, 'max_parallel is positive');
  }
  assert.ok(schemas[0] === schemas[1], 'source and installed schemas are byte-identical');
});
