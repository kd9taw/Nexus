// @vitest-environment jsdom
import {afterEach,expect,it,vi} from 'vitest'
import {BrowserClient,RemoteError} from './client'
import {APPLICATION_VERSIONS} from './application-capabilities'
const auth0=vi.hoisted(()=>({created:vi.fn((_options:unknown)=>{}),redirect:vi.fn(async()=>{}),login:vi.fn(async(_options?:unknown)=>{}),logout:vi.fn(async(_options?:unknown)=>{})}))
vi.mock('@auth0/auth0-spa-js',()=>({Auth0Client:class {constructor(options:unknown){auth0.created(options)} async checkSession(){} handleRedirectCallback(){return auth0.redirect()} loginWithRedirect(options?:unknown){return auth0.login(options)} logout(options?:unknown){return auth0.logout(options)}}}))
afterEach(()=>{vi.unstubAllGlobals();vi.clearAllMocks();window.history.replaceState({},'','/')})
const config=()=>vi.stubGlobal('fetch',vi.fn(async()=>new Response(JSON.stringify({issuer:'https://identity.remote-test.invalid/',audience:'nexus',clientId:'synthetic',ready:true}),{status:200})))
// The live post-login Action's deny text, as Auth0 logs it.
const UNVERIFIED='Please verify your email address using the link we sent, then sign in again.'
async function callback(error:string,description:string|undefined){
  config()
  // spa-js 2.24.1 builds this from the callback's own ?error= and ?error_description= parameters.
  auth0.redirect.mockRejectedValueOnce(Object.assign(new Error(description??error),{error,error_description:description}))
  window.history.replaceState({},'','/?error='+encodeURIComponent(error)+(description===undefined?'':'&error_description='+encodeURIComponent(description))+'&state=synthetic')
  return BrowserClient.load()
}
// Rulings 547 and 548: the page's sign-in asks the account service who signed in and the address it
// verified (how a station knows a person it invited), and never for the profile: no name, nickname
// or picture. spa-js puts this scope in every authorize request, adding only `openid` to it.
it('asks the sign-in for who signed in and their verified address, and nothing of their profile',async()=>{
  config()
  await BrowserClient.load()
  expect(auth0.created).toHaveBeenCalledTimes(1)
  const [options]=auth0.created.mock.calls[0] as [{authorizationParams:{scope:string}}]
  expect(options.authorizationParams.scope).toBe('openid email')
})
it('uses every supported advertised service version at real browser startup and falls back for unknown versions',async()=>{
  for(const version of [...APPLICATION_VERSIONS,undefined,0,17,1.5]){
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response(JSON.stringify({issuer:'https://identity.remote-test.invalid/',audience:'nexus',clientId:'synthetic',ready:true,applicationVersion:version}),{status:200})))
    const client=await BrowserClient.load()
    expect(client?.applicationVersion).toBe(APPLICATION_VERSIONS.find(v=>v===version)??1)
    expect(client?.signInRefusal).toBeNull()
  }
})
// A new password sign-up is logged straight in, and the post-login Action turns it away until the
// address is confirmed. Auth0 then redirects back with ?error=access_denied. That is a sign-up that
// WORKED, so load() must hand back a usable client that says so - throwing left the page with no
// client, which hid every sign-in button and stranded the operator behind a lone "Try again".
it('reads an unconfirmed-email deny on the redirect back as "confirm your email", with a working client',async()=>{
  const client=await callback('access_denied',UNVERIFIED)
  expect(client).toBeInstanceOf(BrowserClient)
  expect(client?.signInRefusal).toBe('emailUnverified')
  expect(window.location.search).toBe('')
  // Auth0 keeps its session through a deny. "Continue" is a plain fresh login (no sign-up screen),
  // which that session completes once the address is confirmed, without the password again...
  await client!.signIn()
  expect(auth0.login).toHaveBeenCalledTimes(1)
  expect(auth0.login).toHaveBeenCalledWith(undefined)
  // ...and "use a different account" has to end that session, or every sign-in bounces straight back.
  expect(auth0.logout).not.toHaveBeenCalled()
  await client!.signOut()
  expect(auth0.logout).toHaveBeenCalledWith({logoutParams:{returnTo:window.location.origin}})
})
it('treats a deny with no usable description, or the stable token, as the unconfirmed-email case',async()=>{
  // Missing entirely; spa-js's own fallback of repeating the code; and the token the Action can
  // move to so rewording its text cannot change what the page shows.
  for(const description of [undefined,'','access_denied','email_unverified']){
    expect((await callback('access_denied',description))?.signInRefusal).toBe('emailUnverified')
    expect(window.location.search).toBe('')
  }
})
it('keeps any other deny a plain refusal, and any other callback failure a failure',async()=>{
  // Control: the tenant can deny for reasons that have nothing to do with email. Those must not
  // tell the operator to go and look in an inbox.
  const blocked=await callback('access_denied','This account is blocked.')
  expect(blocked?.signInRefusal).toBe('signInRefused')
  // Control: a callback that fails for any other reason is not dressed up as a refusal of either kind.
  const other=await callback('missing_transaction','Invalid state').catch((cause:unknown)=>cause)
  expect(other).not.toBeInstanceOf(BrowserClient)
  expect(other instanceof RemoteError&&['signInRefused','emailUnverified'].includes(other.code)).toBe(false)
  expect(window.location.search).toBe('')
})
// Subscribe and Manage show only where the service says its billing is on, by exactly `true`: an older
// service never says, and anything else is not on.
it('turns billing on only for a service that says billing: true',async()=>{
  for(const [billing,on] of [[true,true],[false,false],[undefined,false],[1,false],['true',false]] as const){
    vi.stubGlobal('fetch',vi.fn().mockResolvedValue(new Response(JSON.stringify({issuer:'https://identity.remote-test.invalid/',audience:'nexus',clientId:'synthetic',ready:true,billing}),{status:200})))
    expect((await BrowserClient.load())?.billing).toBe(on)
  }
})
