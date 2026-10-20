const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const {join} = require('node:path');
const {runInNewContext} = require('node:vm');
for (const [kind, ext] of [['pdf','pdf'],['vector','svg']]) {
  const config = {channel:`lumiadam-${kind}-v1`,inputs:[ext],outputs:[ext]};
  function setup(search = '') {
    const messages=[],opened=[];
    let receive, repaints = 0;
    const parent={postMessage:data=>messages.push(data)};
    const window={LUMIADAM_EDITOR:config,addEventListener:(_,fn)=>{receive=fn},lumiadamOpen:(...args)=>opened.push(args),lumiadamRepaint:()=>repaints++};
    runInNewContext(readFileSync(join(__dirname,'../../..','integrations/craft-editors/bridge.js'),'utf8'),
      {window,parent,location:{origin:'http://tauri.localhost',search},URLSearchParams,ArrayBuffer,Uint8Array,crypto:{randomUUID:()=> 'export-id'}});
    window.lumiadamReady();
    const send=(data,origin='http://tauri.localhost',source=parent)=>receive({source,origin,data:{channel:config.channel,...data}});
    return {window,opened,messages,send,get repaints(){return repaints}};
  }
  test(`${kind}: yabancı köken, pencere, kanal ve geçersiz kaynak reddedilir`,()=>{
    const s=setup(),open={type:'open',name:`plan.${ext}`,bytes:new ArrayBuffer(1)};
    s.send(open,'https://example.com'); s.send(open,undefined,{}); s.send({...open,channel:'other'});
    s.send({...open,name:'scene.max'}); s.send({...open,bytes:new ArrayBuffer(0)});
    assert.equal(s.opened.length,0);
    s.send(open); s.send(open); assert.equal(s.opened.length,1);
  });
  test(`${kind}: kayıt onayı beklenir ve hatalı çıktı kaynak dosyasını ezmez`,()=>{
    const s=setup();
    assert.throws(()=>s.window.lumiadamExport(`plan.${ext}`,new Uint8Array([1])),/Open a library/);
    s.send({type:'open',name:`plan.${ext}`,bytes:new ArrayBuffer(1)});
    assert.throws(()=>s.window.lumiadamExport('plan.exe',new Uint8Array([1])),/not enabled/);
    assert.throws(()=>s.window.lumiadamExport(`plan.${ext}`,new Uint8Array()),/256 MB/);
    const bytes=new Uint8Array([1,2]); s.window.lumiadamExport(`plan.${ext}`,bytes); bytes[0]=9;
    assert.equal(new Uint8Array(s.messages.at(-1).bytes)[0],1);
    s.send({type:'saved',id:'wrong'}); assert.throws(()=>s.window.lumiadamExport(`plan.${ext}`,bytes),/pending/);
    s.send({type:'saved',id:'export-id'});
    assert.doesNotThrow(()=>s.window.lumiadamExport(`plan.${ext}`,bytes));
  });
  test(`${kind}: dil değişimi açık belgeyi ve bekleyen dışa aktarımı korur`,()=>{
    const s=setup('?lang=tr');
    assert.equal(s.window.lumiadamLanguage(),'tr');
    assert.throws(()=>s.window.lumiadamExport(`plan.${ext}`,new Uint8Array([1])),/Önce kütüphaneden/);
    s.send({type:'open',name:`Save.${ext}`,bytes:new ArrayBuffer(1)});
    s.window.lumiadamExport(`Save.${ext}`,new Uint8Array([1]));
    s.send({type:'language',language:'en'});
    assert.equal(s.window.lumiadamLanguage(),'en');
    assert.equal(s.repaints,1);
    assert.equal(s.opened.length,1);
    assert.equal(s.opened[0][0],`Save.${ext}`);
    assert.throws(()=>s.window.lumiadamExport(`Save.${ext}`,new Uint8Array([1])),/pending/);
    s.send({type:'language',language:'tr'});
    s.send({type:'open',name:`another.${ext}`,bytes:new ArrayBuffer(1)});
    assert.equal(s.opened.length,1);
    s.send({type:'saved',id:'export-id'});
    assert.doesNotThrow(()=>s.window.lumiadamExport(`Save.${ext}`,new Uint8Array([1])));
  });
  test(`${kind}: yetkisiz ve desteklenmeyen dil mesajları yok sayılır`,()=>{
    const s=setup('?lang=unknown'), change={type:'language',language:'tr'};
    s.send(change,'https://example.com');s.send(change,undefined,{});s.send({...change,channel:'other'});
    s.send({...change,language:'de'});s.send({...change,language:{value:'tr'}});
    assert.equal(s.window.lumiadamLanguage(),'en');
    assert.equal(s.repaints,0);
    s.send(change);s.window.lumiadamNotice();
    assert.equal(s.window.lumiadamLanguage(),'tr');
    assert.match(s.messages.at(-1).message,/Kütüphaneler/);
  });
}
