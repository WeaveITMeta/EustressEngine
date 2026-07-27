const { chromium, devices } = require('playwright');
(async()=>{const b=await chromium.launch();const c=await b.newContext({...devices['Pixel 5']});const p=await c.newPage();
await p.goto('http://127.0.0.1:3001/docs/getting-started',{waitUntil:'domcontentloaded'});
await p.waitForSelector('.page',{timeout:15000}).catch(()=>{});await p.waitForTimeout(3000);
const r=await p.evaluate(()=>{const q=s=>{const e=document.querySelector(s);if(!e)return null;const cs=getComputedStyle(e);const b=e.getBoundingClientRect();return{w:Math.round(b.width),display:cs.display,flexDir:cs.flexDirection,minW:cs.minWidth,width:cs.width}};
return{vw:document.documentElement.clientWidth,layout:q('.docs-layout'),toc:q('.docs-toc'),content:q('.docs-content')};});
console.log(JSON.stringify(r,null,1));await b.close();})();
