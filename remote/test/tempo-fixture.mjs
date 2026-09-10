// Synthetic chat data for the compiled browser and actual native reader.
export function tempoConversations(tier, extra = 0) {
  const message = (text, patch = {}) => ({ from:'N0CALL',to:'W1AW',text,slot:1,directedToMe:false,
    outbound:true,snr:-8,freqHz:1500,dtSec:0.1,tier,incomplete:null,delivered:false,ackId:null,
    attempts:0,confirmed:false,noAck:false,stored:false,abandoned:false,...patch })
  return [
    {peer:'W1AW',messages:[message('HELD',{stored:true}),message('SENDING',{attempts:2}),
      message('CONFIRMED',{confirmed:true}),message('DELIVERED',{delivered:true}),message('NO ACK',{noAck:true}),
      message('ABANDONED',{abandoned:true}),message('PARTIAL',{from:'W1AW',outbound:false,incomplete:[2,3]}),
      message('LEGACY',{from:'W1AW',outbound:false,tier:null}),
      ...Array.from({length:extra},(_,i)=>message(`CHAT HISTORY ${i+1}`,{slot:i+2,outbound:false,from:'W1AW'}))]},
    {peer:'K2ABC',messages:[message('SECOND THREAD',{from:'K2ABC',outbound:false,tier:null})]},
    {peer:'*',messages:[message('BAND MESSAGE',{outbound:false,to:null})]},
  ]
}
