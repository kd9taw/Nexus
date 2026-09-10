// Synthetic receive-only pictures and positions for real browser layout checks.
import { readFile } from 'node:fs/promises'
import { createHash, randomUUID } from 'node:crypto'
import { deflateSync } from 'node:zlib'
function crc32(bytes) {
  let crc=0xffffffff
  for(const byte of bytes){crc^=byte;for(let i=0;i<8;i++)crc=(crc>>>1)^((crc&1)?0xedb88320:0)}
  return (crc^0xffffffff)>>>0
}
function pngChunk(name,data){
  const kind=Buffer.from(name),size=Buffer.alloc(4),crc=Buffer.alloc(4)
  size.writeUInt32BE(data.length);crc.writeUInt32BE(crc32(Buffer.concat([kind,data])))
  return Buffer.concat([size,kind,data,crc])
}
export function picture(){
  const width=320,height=256,header=Buffer.alloc(13),pixels=Buffer.alloc((width*3+1)*height)
  header.writeUInt32BE(width);header.writeUInt32BE(height,4);header[8]=8;header[9]=2
  for(let y=0;y<height;y++)for(let x=0;x<width;x++){
    const at=y*(width*3+1)+1+x*3
    pixels[at]=Math.round(x/width*220);pixels[at+1]=Math.round(y/height*220);pixels[at+2]=180
  }
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),pngChunk('IHDR',header),pngChunk('IDAT',deflateSync(pixels)),pngChunk('IEND',Buffer.alloc(0))])
}
export async function stationModesFixture(){
  const load=async name=>JSON.parse(await readFile(new URL(`../../ui/src/remote-web/__fixtures__/${name}.json`,import.meta.url),'utf8'))
  const [sstv,aprs,roster]=await Promise.all(['sstv','aprs','aprs-roster'].map(load))
  const now=Date.now(),delta=Math.floor((now-aprs.capturedAtMs)/1000)
  sstv.capturedAtMs=now;aprs.capturedAtMs=now;roster.meta.capturedAtMs=now
  for(const [object,keys] of [[sstv.state.health,['lastAudioUnix','lastVisUnix','lastImageUnix']],[aprs.health,['lastAudioUnix','lastDecodeUnix','lastFrameSeenUnix']],[aprs.isStatus,['lastPacketUnix']]]){
    for(const key of keys)if(object[key]!==null)object[key]+=delta
  }
  for(const row of roster.rows)for(const key of ['atUnix','lastHeardUnix','lastRfUnix','lastInetUnix','firstHeardUnix'])if(row.value[key]!=null)row.value[key]+=delta
  const packets=roster.rows.slice(0,3),stations=roster.rows.slice(3)
  for(let i=0;i<37;i++){
    packets.push({kind:'packet',value:{...packets[0].value,source:`K${i}LAY`,text:`APRS packet ${i}`,atUnix:Math.floor(now/1000)-i}})
    stations.push({kind:'station',value:{...stations[0].value,call:`K${i}LAY`,text:`APRS station ${i}`,lat:41.5+i/10,lon:-72.5+i/10}})
  }
  roster.rows=[...packets,...stations];roster.meta.packets=packets.length;roster.meta.stations=stations.length
  const bytes=picture(),images=new Map(),gallery=[]
  for(let i=0;i<40;i++){
    const id=randomUUID()+'.png';gallery.push({...sstv.state.gallery[0],path:id,fskId:i?'K'+i+'TEST':'W1AW'})
    const rows=Array.from({length:Math.ceil(bytes.length/(48*1024))},(_,index)=>({index,base64:bytes.subarray(index*48*1024,(index+1)*48*1024).toString('base64')}))
    images.set(id,{rows,meta:{imageId:id,mime:'image/png',byteLength:bytes.length,sha256:createHash('sha256').update(bytes).digest('hex'),width:320,height:256}})
  }
  sstv.state.gallery=gallery
  return {sstv,aprs,roster,images}
}
