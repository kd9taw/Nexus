// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest'
import {BrowserClient} from './client'
import {APPLICATION_VERSIONS} from './application-capabilities'
vi.mock('@auth0/auth0-spa-js',()=>({Auth0Client:class {async checkSession(){}}}))
afterEach(()=>vi.unstubAllGlobals())
it('uses every supported advertised service version at real browser startup and falls back for unknown versions',async()=>{
  for(const version of [...APPLICATION_VERSIONS,undefined,0,15,1.5]){
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response(JSON.stringify({issuer:'https://identity.remote-test.invalid/',audience:'nexus',clientId:'synthetic',ready:true,applicationVersion:version}),{status:200})))
    const client=await BrowserClient.load()
    expect(client?.applicationVersion).toBe(APPLICATION_VERSIONS.find(v=>v===version)??1)
  }
})
