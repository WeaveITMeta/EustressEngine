import test from 'node:test';
import assert from 'node:assert/strict';
import { validateAvatar, handleAvatar, ROBOT_HEIGHT, ROBOT_BUILD } from '../src/avatar.mjs';

function avatar(identity='R') {
  const body = identity==='R' ? {height:ROBOT_HEIGHT,build:ROBOT_BUILD} : {height:.5,build:.5};
  return { schema_version:2, identity, base_body: {M:'masculine',F:'feminine',R:'robot'}[identity], rig:null,
    morphs:{...body,leg_ratio:.5,face_round:1,face_square:0,face_oval:0,face_diamond:0},
    palette:{skin:'#f5d0a9',hair:'#1a1a1a',top:'#2e5c8a',bottom:'#262633'},slots:[],motion:{} };
}
const json=(value,status,headers)=>new Response(JSON.stringify(value),{status,headers});
function environment() {
  const store=new Map(), kyc=new Map();
  const kv=m=>({get:async key=>m.get(key)??null,put:async(key,value)=>m.set(key,value)});
  return { USERS:kv(store), KYC_STATUS:kv(kyc), store, kyc };
}
// A linked front-of-document record, as registration leaves it. `extracted_sex`
// is what the gate reads; `status` is what makes it eligible to be read.
const verify=(env,userId,extracted_sex,status='linked')=>env.kyc.set(`kyc-${userId}-front`,JSON.stringify({status,extracted_sex,extracted_dob:'1990-01-01'}));
// An agent account: an AI model registered in its own right. No document, ever.
const agent=(env,userId)=>env.store.set(`user:${userId}`,JSON.stringify({id:userId,username:userId,account_type:'agent'}));
const request=(method,body)=>new Request('https://api.eustress.dev/api/avatar',{method,...(body===undefined?{}:{body:JSON.stringify(body)})});

test('only fixed M, F, R choices with their matching body are accepted',()=>{
  for(const id of ['M','F','R']) assert.equal(validateAvatar(avatar(id)).identity,id);
  for(const id of ['male','fluid','','__proto__',null]) assert.throws(()=>validateAvatar({...avatar(),identity:id}));
  assert.throws(()=>validateAvatar({...avatar(),base_body:'masculine'}));
});
test('invalid rig paths and duplicate aliases are rejected',()=>{
  const rig={id:'voltec_supreme',label:'Voltec Supreme',identity:'R',body_asset:'bundled://characters/voltec_supreme.glb',
    animations:['idle','walking','running','jump'].map(m=>`bundled://characters/animations/robot_${m}.glb`),bone_aliases:[]};
  validateAvatar({...avatar(),rig});
  assert.throws(()=>validateAvatar({...avatar(),rig:{...rig,id:undefined}}));
  for(const body_asset of ['bundled://characters/../secret.glb','https://example.com/a.glb','bundled://characters/a.glb#Scene0']) {
    assert.throws(()=>validateAvatar({...avatar(),rig:{...rig,body_asset}}));
  }
  assert.throws(()=>validateAvatar({...avatar(),rig:{...rig,bone_aliases:[['A','hips'],['B','hips']]}}));
});
test('account saves round trip and are isolated by authenticated account',async()=>{
  const env=environment(); const authA=async()=> 'a',authB=async()=> 'b';
  verify(env,'a','F'); verify(env,'b','M');
  assert.equal((await handleAvatar(request('PUT',avatar('F')),env,{},authA,json)).status,200);
  const saved=await handleAvatar(request('GET'),env,{},authA,json);
  assert.deepEqual((await saved.json()).descriptor,avatar('F'));
  assert.equal((await (await handleAvatar(request('GET'),env,{},authB,json)).json()).descriptor,null);
  assert.equal(saved.headers.get('Cache-Control'),'private, no-store');
});
test('unauthenticated and oversized writes never persist',async()=>{
  const env=environment(); verify(env,'a','M');
  assert.equal((await handleAvatar(request('PUT',avatar('M')),env,{},async()=>null,json)).status,401);
  assert.equal((await handleAvatar(request('PUT',{...avatar('M'),padding:'x'.repeat(33000)}),env,{},async()=>'a',json)).status,413);
  assert.equal((await handleAvatar(request('PUT',{...avatar('M'),identity:'Q'}),env,{},async()=>'a',json)).status,400);
  assert.equal(env.store.size,0);
});

test('an account with no usable sex marker cannot customise at all',async()=>{
  // Three populations, one answer: no record, a legacy record from before the
  // field existed, and an X marker. GET says so; PUT refuses; nothing persists.
  for(const seed of [null,{status:'linked'},{status:'linked',extracted_sex:'X'},{status:'linked',extracted_sex:''}]){
    const env=environment(); const auth=async()=>'a';
    if(seed) env.kyc.set('kyc-a-front',JSON.stringify(seed));
    assert.equal((await (await handleAvatar(request('GET'),env,{},auth,json)).json()).locked_identity,null);
    const put=await handleAvatar(request('PUT',avatar('R')),env,{},auth,json);
    assert.equal(put.status,403); assert.equal((await put.json()).code,'kyc_sex_required');
    assert.equal(env.store.size,0,`persisted for seed ${JSON.stringify(seed)}`);
  }
});
test('a marker on a record that never passed verification does not count',async()=>{
  const env=environment(); verify(env,'a','F','rejected');
  assert.equal((await handleAvatar(request('PUT',avatar('F')),env,{},async()=>'a',json)).status,403);
  assert.equal(env.store.size,0);
});
test('a human is exactly the sex on the document; neither the other sex nor Robot',async()=>{
  const env=environment(); const auth=async()=>'a'; verify(env,'a','F');
  const seen=await (await handleAvatar(request('GET'),env,{},auth,json)).json();
  assert.equal(seen.locked_identity,'F'); assert.equal(seen.lock_source,'document');
  assert.equal((await handleAvatar(request('PUT',avatar('F')),env,{},auth,json)).status,200);
  for(const other of ['M','R']){
    const refused=await handleAvatar(request('PUT',avatar(other)),env,{},auth,json);
    assert.equal(refused.status,403,other); assert.equal((await refused.json()).code,'identity_locked');
  }
  // The refused writes did not clobber the accepted one.
  assert.equal((await (await handleAvatar(request('GET'),env,{},auth,json)).json()).descriptor.identity,'F');
});
test('an agent account is Robot, needs no document, and cannot be a sex',async()=>{
  const env=environment(); const auth=async()=>'bot'; agent(env,'bot');
  const seen=await (await handleAvatar(request('GET'),env,{},auth,json)).json();
  assert.equal(seen.locked_identity,'R'); assert.equal(seen.lock_source,'agent');
  assert.equal((await handleAvatar(request('PUT',avatar('R')),env,{},auth,json)).status,200);
  for(const sex of ['M','F']) assert.equal((await handleAvatar(request('PUT',avatar(sex)),env,{},auth,json)).status,403);
  // A document on an agent account changes nothing: the account type wins.
  verify(env,'bot','M');
  assert.equal((await (await handleAvatar(request('GET'),env,{},auth,json)).json()).locked_identity,'R');
  assert.equal((await handleAvatar(request('PUT',avatar('M')),env,{},auth,json)).status,403);
});
test('a user record without account_type is a human',async()=>{
  const env=environment(); env.store.set('user:a',JSON.stringify({id:'a',username:'a'})); verify(env,'a','M');
  assert.equal((await (await handleAvatar(request('GET'),env,{},async()=>'a',json)).json()).locked_identity,'M');
  assert.equal((await handleAvatar(request('PUT',avatar('R')),env,{},async()=>'a',json)).status,403);
});

test('a Robot body is fixed: any other height or build is refused, humans stay free',()=>{
  assert.equal(validateAvatar(avatar('R')).identity,'R');
  for(const morphs of [{height:.5},{build:.7},{height:ROBOT_HEIGHT+0.01},{height:0},{build:1}])
    assert.throws(()=>validateAvatar({...avatar('R'),morphs:{...avatar('R').morphs,...morphs}}),/fixed height and build/,JSON.stringify(morphs));
  // A JSON round trip of the f32 the client sends must still pass.
  assert.equal(validateAvatar({...avatar('R'),morphs:{...avatar('R').morphs,height:Math.fround(ROBOT_HEIGHT)}}).identity,'R');
  // Humans keep the sliders.
  for(const id of ['M','F']) assert.equal(validateAvatar({...avatar(id),morphs:{...avatar(id).morphs,height:.9,build:.1}}).identity,id);
});
