'use strict';
// Exercise the complete shipped bundle and its real navigation/visibility
// callbacks. Only DOM, timers and HTTP are deterministic offline substitutes.
const {test}=require('node:test');
const assert=require('node:assert/strict');
const {sandbox,project,respond,flush}=require('./observatory.cjs');
const A='TASK-'+'1'.repeat(20)+'-'+'a'.repeat(32);
const B='TASK-'+'2'.repeat(20)+'-'+'b'.repeat(32);
function job(id=A,patch={}){
  const stream=()=>({text:'working output\n',total_bytes:15,truncated:false,redacted:false});
  return {schema_version:1,kind:'command_job',workspace:'A',task_id:id,tool:'run_command',origin:'ui',status:'working',can_cancel:true,
    success:null,exit_code:null,error:null,completion_is_not_success:true,acceptance_ready:false,stdout:stream(),stderr:stream(),...patch};
}
async function prepared(){
  const s=sandbox(false,true,{fakeTimers:true});
  s.context.fixtureProject=project();s.context.fixtureJob=job();
  s.run('state.project=fixtureProject;state.workspaceTab="activity";state.autoRefresh=false;clearRuntimeJobs();state.runtimeJobs=[fixtureJob];state.runtimeJobsLoaded=true;');
  const pending=s.run('observeRuntimeJob(fixtureJob.task_id)');
  await flush();assert.equal(s.requests.length,1);respond(s.requests[0],job());await pending;
  assert.equal(s.run('state.runtimeJobsPolls'),1);assert.ok(currentTimer(s));
  return s;
}
const jobRequests=s=>s.requests.filter(request=>request.url.startsWith('/intelligence/jobs/'));
const currentTimer=s=>s.timers.get(s.run('state.runtimeJobsTimer'));
const transitions={
  activity:{leave:s=>s.run('activateWorkspaceTab("overview")'),resume:s=>s.run('activateWorkspaceTab("activity")')},
  visibility:{leave:s=>{s.context.document.hidden=true;s.events.visibilitychange();},resume:s=>{s.context.document.hidden=false;s.events.visibilitychange();}},
};
async function leaveAndExpire(s,transition){
  const id=s.run('state.runtimeJobsTimer'),queued=currentTimer(s),before=jobRequests(s).length;
  assert.ok(queued);transition.leave(s);
  // The event may already have been queued when clearTimeout ran.
  s.timers.delete(id);queued.fn();await flush();
  assert.equal(jobRequests(s).length,before,'hidden/inactive callbacks must not read');
  return queued;
}
async function readNext(s,id=A,response=job(id)){
  const timer=currentTimer(s),before=jobRequests(s).length;
  assert.ok(timer,'LIFECYCLE_RETURN_MUST_RESUME_SELECTED_JOB');assert.equal(timer.ms,2000);
  s.timers.delete(s.run('state.runtimeJobsTimer'));timer.fn();await flush();
  assert.equal(jobRequests(s).length,before+1);
  const request=jobRequests(s).at(-1);
  assert.equal(request.url,'/intelligence/jobs/'+id);assert.equal(request.options.method,'GET');
  assert.equal(request.options.body,undefined);respond(request,response);await flush();
}
test('returning to Activity resumes exact selected Job observation and preserves output inspection',async()=>{
  const s=await prepared();s.run('state.runtimeJobsPolls=41;setRuntimeJobOutput("working","stdout");');
  const view=s.run('state.runtimeJobOutput');
  await leaveAndExpire(s,transitions.activity);transitions.activity.resume(s);
  assert.equal(s.run('state.runtimeJobsPolls'),41,'navigation must preserve remaining budget');
  assert.equal(s.run('state.runtimeJobOutput'),view);assert.equal(view.query,'working');assert.equal(view.stream,'stdout');
  await readNext(s);assert.equal(s.run('state.runtimeJobsPolls'),42);
});
test('a visible document resumes selected Job observation after its hidden timer was delivered',async()=>{
  const s=await prepared();await leaveAndExpire(s,transitions.visibility);transitions.visibility.resume(s);
  assert.equal(s.run('state.runtimeJobsPolls'),1,'visibility must not reset budget');
  await readNext(s);assert.equal(s.run('state.runtimeJobsPolls'),2);
});
test('normal project and activity refresh on visible return cannot strand the selected Job observer',async()=>{
  const s=await prepared();await leaveAndExpire(s,transitions.visibility);
  s.run('state.autoRefresh=true;state.revisionKey="same|||";');transitions.visibility.resume(s);await flush();
  const revision=s.requests.find(request=>request.url==='/intelligence/revision');
  const activity=s.requests.find(request=>request.url==='/intelligence/activity');
  assert.ok(revision);assert.ok(activity);
  respond(revision,{workspace:'A',fingerprint:'same'});
  respond(activity,{workspace:'A',pending_authorizations:0,activity:{available:true,recent:[],active:1,queued:0}});
  await flush();await flush();
  assert.equal(s.run('state.projectTickActive'),false);assert.equal(s.run('state.activityTickActive'),false);
  assert.ok(!s.requests.some(request=>request.url==='/intelligence/jobs'),'background refresh must not discover or launch Jobs');
  await readNext(s);
});
test('a working exact-Job response that settles while hidden resumes only when visible',async()=>{
  const s=await prepared(),pending=s.run('observeRuntimeJob()');await flush();
  transitions.visibility.leave(s);respond(jobRequests(s).at(-1),job());await pending;
  assert.equal(s.run('state.runtimeJobsBusy'),false);assert.equal(currentTimer(s),undefined);
  transitions.visibility.resume(s);await readNext(s);
});
test('lifecycle return preserves busy, error, uncertainty, outcome and observation-budget guards',async()=>{
  for(const transition of Object.values(transitions))for(const change of [
    'state.runtimeJobsBusy=true',
    'state.runtimeJobsMessage="status unavailable"',
    'state.runtimeJobsUncertain=true',
    'state.runtimeJob.status="completed";state.runtimeJob.can_cancel=false',
    'state.runtimeJob.status="input_required";state.runtimeJob.can_cancel=false',
    'state.runtimeJob.status="unknown";state.runtimeJob.can_cancel=false',
    'state.runtimeJobsPolls=120',
    'state.runtimeJob=null;state.runtimeJobSelected=""',
  ]){
    const s=await prepared();await leaveAndExpire(s,transition);s.run(change);const before=s.run('state.runtimeJobsPolls');
    transition.resume(s);await flush();assert.equal(currentTimer(s),undefined,change);
    assert.equal(jobRequests(s).length,1,change);assert.equal(s.run('state.runtimeJobsPolls'),before,change);
  }
});
test('lifecycle return clears obsolete workspace, owner and epoch before considering a retained Job',async()=>{
  for(const transition of Object.values(transitions))for(const change of [
    'state.current="B"',
    'state.runtimeJobsScope={...state.runtimeJobsScope,owner:"previous-owner"}',
    'state.workspaceEpoch++',
  ]){
    const s=await prepared(),queued=await leaveAndExpire(s,transition);s.run(change);transition.resume(s);queued.fn();await flush();
    assert.equal(s.run('state.runtimeJob'),null,change);assert.equal(s.run('state.runtimeJobSelected'),'',change);
    assert.equal(currentTimer(s),undefined,change);assert.equal(jobRequests(s).length,1,change);
  }
});
test('repeated returns keep one observer and an old queued callback cannot retarget a replacement Job',async()=>{
  const s=await prepared(),queued=await leaveAndExpire(s,transitions.activity);
  s.context.replacement=job(B);s.run('state.runtimeJobSelected=replacement.task_id;state.runtimeJob=replacement;');
  transitions.activity.resume(s);transitions.activity.resume(s);s.events.visibilitychange();
  assert.equal(s.timers.size,1,'only one exact-Job timer may remain');queued.fn();await flush();
  assert.equal(jobRequests(s).length,1,'old selection must not read replacement');
  await readNext(s,B);assert.equal(s.run('state.runtimeJob.task_id'),B);
});
test('lifecycle return preserves the final automatic observation and requires explicit budget renewal',async()=>{
  const s=await prepared();s.run('state.runtimeJobsPolls=119;');await leaveAndExpire(s,transitions.visibility);transitions.visibility.resume(s);
  await readNext(s);assert.equal(s.run('state.runtimeJobsPolls'),120);assert.equal(currentTimer(s),undefined);
  transitions.activity.leave(s);transitions.activity.resume(s);s.events.visibilitychange();
  assert.equal(currentTimer(s),undefined);assert.equal(jobRequests(s).length,2);
  const pending=s.run('observeRuntimeJob()');await flush();respond(jobRequests(s).at(-1),job());await pending;
  assert.equal(s.run('state.runtimeJobsPolls'),1);assert.ok(currentTimer(s));
  assert.ok(s.requests.every(request=>request.options.method==='GET'));
});
