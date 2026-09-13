// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest'
import {BrowserClient,RemoteError} from './client'
import {APPLICATION_VERSIONS} from './application-capabilities'
const auth0=vi.hoisted(()=>({redirect:vi.fn(async()=>{})}))
vi.mock('@auth0/auth0-spa-js',()=>({Auth0Client:class {async checkSession(){} handleRedirectCallback(){return auth0.redirect()}}}))
afterEach(()=>{vi.unstubAllGlobals();window.history.replaceState({},'','/')})
it('uses every supported advertised service version at real browser startup and falls back for unknown versions',async()=>{
  for(const version of [...APPLICATION_VERSIONS,undefined,0,15,1.5]){
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response(JSON.stringify({issuer:'https://identity.remote-test.invalid/',audience:'nexus',clientId:'synthetic',ready:true,applicationVersion:version}),{status:200})))
    const client=await BrowserClient.load()
    expect(client?.applicationVersion).toBe(APPLICATION_VERSIONS.find(v=>v===version)??1)
  }
})
// The account service refuses a sign-in by redirecting back with ?error=access_denied - today that
// is the login Action turning away an address nobody has confirmed. Unnamed, it reached the page as
// "check the connection and your service access", which sent a brand-new ham looking for a network
// fault instead of their inbox.
it('names a sign-in the account service refused, and still clears the callback from the address bar',async()=>{
  vi.stubGlobal('fetch',vi.fn(async()=>new Response(JSON.stringify({issuer:'https://identity.remote-test.invalid/',audience:'nexus',clientId:'synthetic',ready:true}),{status:200})))
  const refused=Object.assign(new Error('Please verify your email address using the link we sent, then sign in again.'),{error:'access_denied',error_description:'Please verify your email address using the link we sent, then sign in again.'})
  auth0.redirect.mockRejectedValueOnce(refused)
  window.history.replaceState({},'','/?error=access_denied&error_description=Please%20verify&state=synthetic')
  const failure=await BrowserClient.load().catch((cause:unknown)=>cause)
  expect(failure).toBeInstanceOf(RemoteError)
  expect((failure as RemoteError).code).toBe('signInRefused')
  expect(window.location.search).toBe('')
  // Control: a callback that fails for any other reason must not be dressed up as a refusal.
  auth0.redirect.mockRejectedValueOnce(Object.assign(new Error('Invalid state'),{error:'missing_transaction',error_description:'Invalid state'}))
  window.history.replaceState({},'','/?code=synthetic&state=synthetic')
  const other=await BrowserClient.load().catch((cause:unknown)=>cause)
  expect(other instanceof RemoteError&&other.code==='signInRefused').toBe(false)
  expect(window.location.search).toBe('')
})
