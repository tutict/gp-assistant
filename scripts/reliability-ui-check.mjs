#!/usr/bin/env node
// Browser plugin not available. Real Chromium + explicitly simulated native IPC; not device acceptance.
import {createServer} from 'node:http';
import {readFile, mkdir, writeFile} from 'node:fs/promises';
import {resolve, sep, extname} from 'node:path';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import assert from 'node:assert/strict';
const require=createRequire(new URL('../desktop/frontend/package.json',import.meta.url));
const {chromium}=require('playwright');
const root=fileURLToPath(new URL('../',import.meta.url));
const built=resolve(root,'desktop/mobile-dist');
const out=resolve(root,'tmp/reliability-ui'); await mkdir(out,{recursive:true});
const server=createServer(async(req,res)=>{
  try { const url=new URL(req.url,'http://localhost'); const path=resolve(built,'.'+decodeURIComponent(url.pathname==='/'?'/index.html':url.pathname));
    if(!path.startsWith(built+sep)) {res.writeHead(403).end();return;}
    const mime={'.html':'text/html','.js':'text/javascript','.css':'text/css','.json':'application/json','.woff2':'font/woff2','.svg':'image/svg+xml','.png':'image/png'}[extname(path)]||'application/octet-stream';
    const data=await readFile(path);res.writeHead(200,{'content-type':mime});res.end(data);
  } catch {res.writeHead(404).end();}
});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
const origin='http://127.0.0.1:'+server.address().port;
const browser=await chromium.launch({headless:true}); const report=[];
try {
  for(const viewport of [{width:1440,height:900},{width:390,height:844}]) {
    let workspace={schemaVersion:1,revision:0,values:{}}; let failCommit=false; let enabled=false;
    const calls=[]; const errors=[];
    const context=await browser.newContext({viewport,userAgent:viewport.width<500?'Mozilla/5.0 Android GuXuanYou QA':'Mozilla/5.0 Windows GuXuanYou QA'});
    const page=await context.newPage(); page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
    await context.route('**/*',route=>route.request().url().startsWith(origin)?route.continue():route.abort());
    const feature=()=>({gepa_requested_enabled:enabled,gepa_effective_enabled:enabled,gepa_compiled:true,safe_start:false});
    await page.exposeBinding('__reliabilityNative',async(_,command,args={})=>{
      calls.push(command); const p=args.payload??{};
      switch(command) {
        case 'api_workspace_load':return structuredClone(workspace);
        case 'api_workspace_commit':if(failCommit)throw Error('simulated storage unavailable');if(p.expectedRevision!==workspace.revision)throw Error('revision conflict');workspace={...workspace,revision:workspace.revision+1,values:{...workspace.values,...p.changes}};return workspace.revision;
        case 'api_watchlist_snapshot':return {items:[],revision:0,migrationComplete:true};
        case 'api_watchlist_list':return [];
        case 'api_market_status':return {universe_count:5231,cache_bytes:67108864,quote_trade_date:'20261006',current_trade_date:'20261006',stale:false};
        case 'api_agent_gepa_status':return {enabled};
        case 'api_diagnostics_status':return feature();
        case 'api_diagnostics_set_gepa':enabled=p.enabled;return feature();
        case 'api_diagnostics_preview':return {schema_version:1,previous_exit:'clean',settings:feature(),events:['started'],counters:{started:1},dropped_events:0};
        case 'api_app_close_handler_ready':return null;
        case 'api_health':return {status:'ok',runtime:'tauri',warnings:[]};
        case 'api_agent_runs':case 'api_agent_runs_list':case 'api_agent_conversations':return {items:[]};
        case 'plugin:event|listen':return 1;case 'plugin:event|unlisten':return null;
        default:throw Error('Unmocked native command: '+command);
      }
    });
    await page.addInitScript(()=>{
      const invoke=(cmd,args)=>window.__reliabilityNative(cmd,args);
      window.__TAURI_INTERNALS__={invoke,transformCallback:()=>1};
      window.__TAURI__={core:{invoke},event:{listen:async()=>()=>{}}};
    });
    const navigate=async(name)=>{
      const nav=page.getByRole('navigation',{name:'主导航'});
      if(!await nav.isVisible())await page.getByRole('button',{name:'打开导航',exact:true}).click();
      await nav.getByRole('link',{name,exact:true}).click();
    };
    await page.goto(origin); await page.locator('.screen-panel-container').waitFor();
    assert.match(await page.title(),/股选优/);
    await navigate('研究助手');
    const composer=page.locator('textarea').filter({visible:true}).first();
    await composer.fill('离线草稿：保留这段文字，不自动发送。');
    await page.waitForFunction(()=>document.body.innerText.includes('已保存到本机'));
    await page.waitForTimeout(500);
    assert(Object.entries(workspace.values).some(([k,v])=>k.startsWith('agent.draft:')&&v==='离线草稿：保留这段文字，不自动发送。'));
    failCommit=true; await composer.fill('保存失败后仍保留的草稿。');
    await page.getByRole('button',{name:'重试保存',exact:true}).first().waitFor();
    failCommit=false; await page.getByRole('button',{name:'重试保存',exact:true}).first().click();
    await page.waitForFunction(()=>document.body.innerText.includes('已保存到本机'));
    await page.goto(origin); await page.locator('.agent-workspace').waitFor();
    assert.equal(await page.locator('textarea').filter({visible:true}).first().inputValue(),'保存失败后仍保留的草稿。');
    await page.screenshot({path:resolve(out,'agent-'+viewport.width+'.png')});
    await page.getByRole('button',{name:'设置',exact:true}).click();
    const toggle=page.getByRole('switch',{name:'启用 GEPA 实验'}); await toggle.click();
    await page.waitForFunction(()=>document.querySelector('[aria-label="启用 GEPA 实验"]')?.getAttribute('aria-checked')==='true');
    await page.getByRole('button',{name:'预览本地诊断'}).click();
    await page.locator('[aria-label="诊断导出预览"]').waitFor();
    await page.screenshot({path:resolve(out,'settings-'+viewport.width+'.png')});
    await page.getByRole('button',{name:'关闭设置',exact:true}).click();
    await navigate('回测'); await page.locator('#btStart').fill('2021-02-03'); await page.locator('#btCostBps').fill('25');
    await page.waitForTimeout(700); await page.goto(origin); await page.locator('#btCostBps').waitFor();
    assert.equal(await page.locator('#btCostBps').inputValue(),'25'); assert.equal(await page.locator('#btStart').inputValue(),'2021-02-03');
    const overflow=await page.evaluate(()=>document.documentElement.scrollWidth>innerWidth+1); assert.equal(overflow,false);
    assert(!calls.includes('api_job_run')&&!calls.includes('api_agent'),'restoring must not submit paid or research jobs');
    assert.equal(errors.length,0,errors.join('\n'));
    report.push({viewport,passed:true,native_backend:'simulated',checks:['draft retry','fresh renderer restore','backtest parameters','feature toggle','diagnostic preview','no auto-submit','no page errors','no horizontal overflow']});
    await context.close();
  }
  await writeFile(resolve(out,'report.json'),JSON.stringify(report,null,2)); console.log(JSON.stringify(report,null,2));
} finally {await browser.close();await new Promise(resolve=>server.close(resolve));}
