'use strict';
// The production-capability matrix keeps every existing engineering surface
// reachable from the home status block. Fitness and the language providers had
// no surface entry, so both could only be found by scrolling the overview page.
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { sandbox, project } = require('./observatory.cjs');

test('the home status surface reaches Fitness and the language providers', () => {
  const s = sandbox();
  s.context.fixture = {
    ...project(),
    fitness: { benchmark: { available: true, status: 'current', partial: false,
      latest: { case_count: 60, profile: 'synthetic' } } },
    language_quality: { languages: [
      { language: 'rust', detected_files: 120, gaps: ['lint'] },
      { language: 'go', detected_files: 8, gaps: [] }] },
  };
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /data-summary-action="fitnessSection"/);
  assert.match(html, /data-summary-action="qualitySection"/);
  assert.match(html, /60 cases/);
  assert.match(html, /2 languages/);
  assert.match(html, /1 gaps/);
});

test('an unavailable benchmark and no detected language are reported as unavailable', () => {
  const s = sandbox();
  s.context.fixture = project();
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /data-summary-action="fitnessSection"/);
  assert.match(html, /No benchmark report available/);
  assert.match(html, /data-summary-action="qualitySection"/);
  assert.match(html, /No detected language files/);
});

test('a partial benchmark is not presented as a current observation', () => {
  const s = sandbox();
  s.context.fixture = { ...project(),
    fitness: { benchmark: { available: true, status: 'historical', partial: true,
      latest: { case_count: 12, profile: 'synthetic' } } } };
  s.run('state.project=fixture;renderObservationCoverage();');
  const html = s.node('#observationCoverage').innerHTML;
  assert.match(html, /Historical or partial/);
});
