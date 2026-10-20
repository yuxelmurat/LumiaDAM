const test=require('node:test'),assert=require('node:assert/strict'),fs=require('node:fs'),path=require('node:path'),vm=require('node:vm'),ts=require('typescript');
const root=path.resolve(__dirname,'../src');
function load(native){ const cache={}, storage=new Map(), document={documentElement:{lang:''}}; const context={console,Intl,Date,Set,Map,RegExp,Error,Promise,document,...(native?{window:{__TAURI_INTERNALS__:{}}}:{}),localStorage:{getItem:k=>storage.get(k),setItem:(k,v)=>storage.set(k,v)}};
function mod(file){ if(cache[file])return cache[file]; const exports={}; cache[file]=exports;const code=ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText; vm.runInNewContext(code,{...context,exports,require:id=>id==='react'?{useSyncExternalStore:(_s,get)=>get()}:id==='@tauri-apps/api/core'?{invoke:native??(async()=>{throw Error('unexpected native call')})}:id.startsWith('.')?mod(path.resolve(path.dirname(file),id+(id.endsWith('.json')?'':'.ts'))):require(id)});return exports; }
return {i:mod(path.join(root,'i18n/index.ts')),document};}
test('Dil seçimi görünür metni değiştirir, dosya yollarını ve bilinmeyen verileri değiştirmez',async()=>{const {i,document}=load();await i.setLanguage('tr');assert.equal(i.t('Libraries'),'Kütüphaneler');assert.equal(i.t('Open {path}',{path:'A:/Kitchen/9.jpg'}),'A:/Kitchen/9.jpg dosyasını aç');assert.equal(i.translateMessage('A:/Kitchen/9.jpg'),'A:/Kitchen/9.jpg');assert.equal(document.documentElement.lang,'tr');await i.setLanguage('en');assert.equal(i.t('Libraries'),'Libraries');});
test('Desteklenmeyen dil reddedilir ve dil değiştirmek aboneleri bilgilendirir',async()=>{const {i}=load();let count=0;const stop=i.subscribeLanguage(()=>count++);await i.setLanguage('tr');await assert.rejects(i.setLanguage('de'));assert.equal(i.getLanguage(),'tr');assert.equal(count,1);stop();await i.setLanguage('en');assert.equal(count,1);});
test('Çalışma çıktıları parametreleri korur; sayı ve tarih seçilen dile göre biçimlenir',async()=>{const {i}=load();await i.setLanguage('tr');assert.equal(i.t('{count} files',{count:19}),'19 dosya');assert.equal(i.formatNumber(1234.5),'1.234,5');assert.equal(i.t('Uncatalogued label'),'Uncatalogued label');});

 test('Kurulumun dili ilk açılışta okunur; kaydetme hatası tercih değiştirilmiş gibi gösterilmez',async()=>{
 const calls=[];const {i,document}=load(async(command,args)=>{calls.push([command,args]);if(command==='get_app_language')return 'tr';throw Error('disk unavailable');});
 await i.initializeLanguage();assert.equal(i.getLanguage(),'tr');assert.equal(document.documentElement.lang,'tr');
 await assert.rejects(i.setLanguage('en'),/disk unavailable/);assert.equal(i.getLanguage(),'tr');
 assert.equal(calls[0][0],'get_app_language');assert.equal(calls[1][0],'set_app_language');assert.equal(calls[1][1].language,'en');
 });
