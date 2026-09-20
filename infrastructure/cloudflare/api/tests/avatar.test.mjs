import test from 'node:test';
import assert from 'node:assert/strict';
import { validateAvatar, handleAvatar } from '../src/avatar.mjs';

function avatar(identity='R') {
  return { schema_version:2, identity, base_body: {M:'masculine',F:'feminine',R:'robot'}[identity], rig:null,
    morphs:{height:.5,build:.5,leg_ratio:.5,face_round:1,face_square:0,face_oval:0,face_diamond:0},
    palette:{skin:'#f5d0a9',hair:'#1a1a1a',top:'#2e5c8a',bottom:'#262633'},slots:[],motion:{} };
}
const json=(value,status,headers)=>new Response(JSON.stringify(value),{status,headers});
function environment() {
  const store=new Map();
  return { USERS:{get:async key=>store.get(key)??null,put:async(key,value)=>store.set(key,value)},store };
}
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
  assert.equal((await handleAvatar(request('PUT',avatar()),env,{},authA,json)).status,200);
  const saved=await handleAvatar(request('GET'),env,{},authA,json);
  assert.deepEqual((await saved.json()).descriptor,avatar());
  assert.equal((await (await handleAvatar(request('GET'),env,{},authB,json)).json()).descriptor,null);
  assert.equal(saved.headers.get('Cache-Control'),'private, no-store');
});
test('unauthenticated and oversized writes never persist',async()=>{
  const env=environment();
  assert.equal((await handleAvatar(request('PUT',avatar()),env,{},async()=>null,json)).status,401);
  assert.equal((await handleAvatar(request('PUT',{...avatar(),padding:'x'.repeat(33000)}),env,{},async()=>'a',json)).status,413);
  assert.equal((await handleAvatar(request('PUT',{...avatar(),identity:'Q'}),env,{},async()=>'a',json)).status,400);
  assert.equal(env.store.size,0);
});
