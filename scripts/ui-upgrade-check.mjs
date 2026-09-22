import { createRequire } from "node:module";
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { installHarnessState, startBuiltAppServer, installAgentReplayState, mockResearchOverviewData, mockResearchMessagesData, mockResearchThreads, mockResearchThreadDetail } from "./ui-screenshot.mjs";

const require = createRequire(new URL("../desktop/frontend/package.json", import.meta.url));
const { chromium } = require("playwright");
const output = resolve(process.argv[2] || fileURLToPath(new URL("../artifacts/ui-upgrade/", import.meta.url)));
mkdirSync(output, { recursive: true });
const research = { overview: mockResearchOverviewData, messages: mockResearchMessagesData, threads: mockResearchThreads, detail: mockResearchThreadDetail };
const routes = [
  { name: "screen", hash: "sectionScreen", ready: ".screen-panel-container" },
  { name: "observe", hash: "sectionObserve", ready: ".observe-panel-container" },
  { name: "backtest", hash: "sectionBacktest", ready: ".backtest-run-button" },
  { name: "news", hash: "sectionNewsRag", ready: ".sentiment-panel" },
  { name: "agent", hash: "sectionAgent", ready: ".agent-workspace" },
];
const configurations = [];
for (const width of [390, 1440]) for (const theme of ["dark", "light"]) for (const density of ["comfortable", "compact"]) {
  configurations.push({width, height: width===390 ? 844 : 900, theme, density, fontScale:"standard"});
}
for (const width of [390,1440]) for (const fontScale of ["small","large"]) configurations.push({width,height:width===390?844:900,theme:"dark",density:"comfortable",fontScale});
for (const width of [360,430,768,769,980,1180,1181,1280,1920,2560]) configurations.push({width,height:width<=768?844:900,theme:"dark",density:"comfortable",fontScale:"standard"});
configurations.push({width:844,height:390,theme:"dark",density:"comfortable",fontScale:"standard",motion:"reduce"});
const report = { generatedAt: new Date().toISOString(), checks: [], failures: [] };
const {server,url}=await startBuiltAppServer();
const browser=await chromium.launch({headless:true});
try {
  for(const config of configurations) {
    const name=`${config.width}x${config.height}-${config.theme}-${config.density}-${config.fontScale}`;
    const context=await browser.newContext({viewport:{width:config.width,height:config.height},deviceScaleFactor:1,hasTouch:config.width<=768,colorScheme:config.theme,reducedMotion:config.motion || "no-preference"});
    const page=await context.newPage();
    const errors=[];
    page.on('pageerror',e=>errors.push(e.message));
    page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
    await installHarnessState(page,undefined,research);
    await installAgentReplayState(page);
    await page.route('**/api/screen',r=>r.fulfill({json:{total:12,returned:12,items:Array.from({length:12},(_,i)=>({stock:{code:`${600000+i}.SH`,name:i===11?'超长股票名称阅读与布局测试':'研究股票'+(i+1),industry:'制造业',price:i===10?null:20+i,change_pct:i%2?-.012:.018,pe:i===9?0:18,pb:2,eps:1.5},score:90-i,balanced_score:90-i,quality_score:80,trend_score:70,risk_score:20,reason_tags:['盈利稳定'],risk_tags:i===11?['估值偏高']:[],explanation:{basis:['盈利质量与趋势共同满足筛选条件。']}}))}}));
    const query=new URLSearchParams({theme:config.theme,density:config.density,fontScale:config.fontScale});
    for(const route of routes) {
      await page.goto(`${url}/?${query}#${route.hash}`,{waitUntil:'networkidle'});
      await page.locator(route.ready).first().waitFor();
      if(route.name==='screen') {await page.locator('.screen-panel-container .run-btn').click();await page.locator('.stock-row').first().waitFor();}
      if(route.name==='observe') {await page.locator('.observe-code-row input').fill('600519.SH');await page.locator('.observe-run-btn').click();await page.locator('.observe-result').waitFor();}
      if(route.name==='backtest') {await page.locator('.backtest-run-button').click();await page.locator('.backtest-result').waitFor();}
      const metrics=await page.evaluate(()=>{
        const visible=e=>{const r=e.getBoundingClientRect();return r.width>0&&r.height>0&&getComputedStyle(e).visibility!=='hidden'&&!e.closest('[hidden], [inert]');};
        const ids=[...document.querySelectorAll('[id]')].map(e=>e.id);
        const hitAreas=[...document.querySelectorAll('button,select,summary,input:not([type=checkbox]):not([type=radio]),textarea,a.nav-link')].filter(visible).filter(e=>!e.closest('svg')).map(e=>({label:e.getAttribute('aria-label')||e.textContent?.trim().slice(0,40),width:e.getBoundingClientRect().width,height:e.getBoundingClientRect().height}));
        return {title:document.title,meaningful:document.querySelector('#root')?.textContent.length>30,overlay:!!document.querySelector('vite-error-overlay'),overflow:document.documentElement.scrollWidth>innerWidth+1,duplicateIds:ids.filter((id,i)=>ids.indexOf(id)!==i),shortTargets:innerWidth<=768?hitAreas.filter(r=>r.width<43.5||r.height<43.5):[],visibleRows:[...document.querySelectorAll('.stock-row')].filter(e=>{const r=e.getBoundingClientRect();return r.top>=0&&r.bottom<=innerHeight-(innerWidth<=768?60:0)}).length};
      });
      const check={configuration:name,route:route.name,...metrics,errors:[...errors]};
      errors.length=0;
      report.checks.push(check);
      const rowTarget = route.name === 'screen' && config.fontScale === 'standard' && [390,1440].includes(config.width) ? config.width === 390 ? 2 : 8 : 0;
      if(metrics.overflow||metrics.overlay||!metrics.meaningful||metrics.duplicateIds.length||metrics.shortTargets.length||check.errors.length||metrics.visibleRows<rowTarget)report.failures.push(check);
      if([390,1440].includes(config.width)) {
        const directory=resolve(output,name);mkdirSync(directory,{recursive:true});
        await page.screenshot({path:resolve(directory,route.name+'.png'),fullPage:false});
      }
    }
    await context.close();
    console.log(`${name}: checked five workspaces`);
  }
} finally {await browser.close();await new Promise(r=>server.close(r));}
writeFileSync(resolve(output,'report.json'),JSON.stringify(report,null,2));
console.log(`${report.checks.length} checks, ${report.failures.length} failures. Report: ${output}`);
if(report.failures.length)process.exitCode=1;
