'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const mode = process.argv[2];
const sourcePath = 'src/ui/intelligence_web/app/engineering.js';
const source = fs.readFileSync(sourcePath, 'utf8');

function load(candidate = source) {
  const context = {
    console,
    Math,
    Number,
    String,
    Date,
    Map,
    Set,
    state: {},
    els: {},
  };
  vm.createContext(context);
  vm.runInContext(candidate, context, { filename: sourcePath });
  return context;
}

function lcg(seed = 0x9e3779b9) {
  let state = seed >>> 0;
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state;
  };
}

function zoomOracle(clamp) {
  const cases = [
    [-999, 0.5],
    [-1, 0.5],
    [0, 1],
    ['', 1],
    [false, 1],
    [null, 1],
    [0.5, 0.5],
    [0.75, 0.75],
    [1, 1],
    [1.35, 1.35],
    [2, 1.35],
    ['1.2', 1.2],
    [Infinity, 1.35],
    [-Infinity, 0.5],
    [NaN, 1],
  ];
  for (const [input, expected] of cases) assert.equal(clamp(input), expected);
}

function propertyStage() {
  const { clampSystemMapScale, engineeringStage } = load();
  zoomOracle(clampSystemMapScale);
  const next = lcg();
  const stages = new Set(['understand', 'change', 'prove', 'observe']);
  for (let index = 0; index < 2048; index++) {
    const value = ((next() % 400001) - 200000) / 1000;
    const once = clampSystemMapScale(value);
    assert.ok(Number.isFinite(once));
    assert.ok(once >= 0.5 && once <= 1.35);
    assert.equal(clampSystemMapScale(once), once, 'zoom clamp must be idempotent');

    const token = `tool_${next().toString(16)}_${index}`;
    assert.ok(stages.has(engineeringStage(token)));
  }
  return { stage: 'property', cases: 2048, invariants: 4 };
}

function mutationStage() {
  const baseline = load();
  zoomOracle(baseline.clampSystemMapScale);

  const mutations = [
    ['Math.min(1.35,', 'Math.min(1.5,'],
    ['Math.max(.5,', 'Math.max(.4,'],
    ['Number(value) || 1', 'Number(value) || 0'],
    ['Math.min(1.35, Math.max', 'Math.max(1.35, Math.max'],
  ];
  let killed = 0;
  for (const [before, after] of mutations) {
    assert.ok(source.includes(before), `mutation anchor disappeared: ${before}`);
    const mutant = source.replace(before, after);
    assert.notEqual(mutant, source);
    const candidate = load(mutant);
    let survived = true;
    try {
      zoomOracle(candidate.clampSystemMapScale);
    } catch {
      survived = false;
    }
    assert.equal(survived, false, `source mutant survived: ${before} -> ${after}`);
    killed++;
  }
  return { stage: 'mutation', mutants: mutations.length, killed };
}

function fuzzValue(next, index) {
  switch (next() % 8) {
    case 0: return null;
    case 1: return Boolean(next() & 1);
    case 2: return ((next() % 2000001) - 1000000) / 100;
    case 3: return `${next() % 99999}.${next() % 1000}`;
    case 4: return ` fuzz_\u0000_${index}_世界_${next().toString(16)} `;
    case 5: return [next() % 10, String(next() % 10)];
    case 6: return { value: next() % 10, nested: { index } };
    default: return [];
  }
}

function fuzzStage() {
  const { clampSystemMapScale, engineeringStage } = load();
  const next = lcg(0x243f6a88);
  const stages = new Set(['understand', 'change', 'prove', 'observe']);
  for (let index = 0; index < 2048; index++) {
    const input = fuzzValue(next, index);
    const zoom = clampSystemMapScale(input);
    assert.ok(Number.isFinite(zoom), `non-finite zoom at fuzz case ${index}`);
    assert.ok(zoom >= 0.5 && zoom <= 1.35, `out-of-range zoom at fuzz case ${index}`);
    const stage = engineeringStage(input);
    assert.ok(stages.has(stage), `invalid engineering stage at fuzz case ${index}`);
  }
  return { stage: 'fuzz', cases: 2048, seed: '0x243f6a88' };
}

const runners = {
  property: propertyStage,
  mutation: mutationStage,
  fuzz: fuzzStage,
};

assert.ok(runners[mode], 'expected property, mutation, or fuzz stage');
const report = runners[mode]();
process.stdout.write(JSON.stringify({ ok: true, ...report }) + '\n');
