//! What Windows' firewall will let in on the network the shack listens on, read at the shack so
//! the operator hears it there before another computer finds the door shut. As ruled on
//! 2026-10-04 ("Windows' own prompt"), Nexus adds no rule of its own: it reads what is there and
//! says what stands in the way ([`says`]).
//!
//! **What is read, and why.**
//! - Windows asks about a program the first time it listens, and the answer becomes rules for that
//!   program on the network types that were ticked. "Cancel" leaves rules that block it, and from
//!   then on nothing asks and nothing says: the rules are read for this program, block before allow,
//!   as Windows applies them.
//! - Windows calls a new network Public until told otherwise, and an answer of "Private networks
//!   only" then keeps every other computer out of a home network that is still called Public: the
//!   network's own category is read from Windows' network list, for the adapter listened on.
//! - A firewall set to block every incoming connection, one whose questions are switched off, and
//!   one run by an administrator's policy each keep other computers out whatever is answered.
//!
//! **What cannot be read from here.** A network that keeps its devices apart, as guest Wi-Fi and
//! some mesh systems do, looks to the shack like any other. The card says so in words, and so does
//! the other computer when nothing answers.

/// A network's category, as the firewall profile it puts in force.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Domain,
    Private,
    Public,
}

impl Profile {
    /// Its bit in a rule's profiles (`NET_FW_PROFILE_TYPE2`).
    pub fn bit(self) -> i32 {
        match self {
            Profile::Domain => 1,
            Profile::Private => 2,
            Profile::Public => 4,
        }
    }
}

/// An inbound rule that names this program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// It allows; otherwise it blocks.
    pub allow: bool,
    pub enabled: bool,
    /// The profiles it is in force on, as bits ([`Profile::bit`]).
    pub profiles: i32,
    /// The IP protocol it covers: 6 TCP, 17 UDP, 256 any.
    pub protocol: i32,
}

const TCP: i32 = 6;
const UDP: i32 = 17;
const ANY: i32 = 256;

/// Windows' firewall on the network the shack listens on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Firewall {
    /// The network's category, which picks the profile below.
    pub profile: Profile,
    /// The firewall is on for that profile.
    pub on: bool,
    /// It blocks every incoming connection there, whatever the rules allow.
    pub block_all: bool,
    /// It asks about a program that listens for the first time; otherwise it blocks it unasked.
    pub asks: bool,
    /// An administrator's policy overrides the rules made on this computer.
    pub managed: bool,
    /// The inbound rules that name this program.
    pub rules: Vec<Rule>,
}

/// What stands in the way, as a code the card puts in words, or `None` when nothing does (the
/// firewall is off there, or it allows this program on this network):
/// - `blocksAll`: it blocks every incoming connection on this network;
/// - `blocked`: a rule blocks this program on this network, as "Cancel" leaves;
/// - `managed`: an administrator's policy decides, and no rule here may count;
/// - `public`: Windows calls this network Public, and nothing allows this program on it;
/// - `silent`: nothing allows it yet, and Windows blocks new programs without asking;
/// - `ask`: nothing allows it yet, and Windows asks (or is asking now).
pub fn says(firewall: &Firewall) -> Option<&'static str> {
    if !firewall.on {
        return None;
    }
    if firewall.block_all {
        return Some("blocksAll");
    }
    let here = |rule: &&Rule, protocols: &[i32]| {
        rule.enabled
            && rule.profiles & firewall.profile.bit() != 0
            && protocols.contains(&rule.protocol)
    };
    if firewall
        .rules
        .iter()
        .any(|rule| !rule.allow && here(&rule, &[TCP, UDP, ANY]))
    {
        return Some("blocked");
    }
    if firewall.managed {
        return Some("managed");
    }
    if firewall
        .rules
        .iter()
        .any(|rule| rule.allow && here(&rule, &[TCP, ANY]))
    {
        return None;
    }
    if firewall.profile == Profile::Public {
        return Some("public");
    }
    Some(if firewall.asks { "ask" } else { "silent" })
}

/// The firewall as Windows has it now for the network on the adapter with this GUID (`{…}`).
/// `None` where it cannot be read: off Windows, or an adapter Windows' network list does not
/// know. Blocks for the time the rules take to read (about a tenth of a second for some 770
/// rules, measured on Windows 11): call it off any thread that must not wait.
pub fn read(adapter: &str) -> Option<Firewall> {
    #[cfg(windows)]
    {
        system::read(adapter)
    }
    #[cfg(not(windows))]
    {
        let _ = adapter;
        None
    }
}

#[cfg(windows)]
mod system {
    use windows::core::{Interface, GUID};
    use windows::Win32::NetworkManagement::WindowsFirewall::{
        INetFwPolicy2, INetFwRule, NetFwPolicy2, NET_FW_ACTION_ALLOW, NET_FW_MODIFY_STATE_OK,
        NET_FW_PROFILE_TYPE2, NET_FW_RULE_DIR_IN,
    };
    use windows::Win32::Networking::NetworkListManager::{
        INetworkListManager, NetworkListManager, NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED,
        NLM_NETWORK_CATEGORY_PRIVATE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };
    use windows::Win32::System::Ole::IEnumVARIANT;
    use windows::Win32::System::Variant::{VARIANT, VT_DISPATCH};

    use super::{Firewall, Profile, Rule};

    pub fn read(adapter: &str) -> Option<Firewall> {
        // SAFETY: COM on this thread for the reads below, undone when it was done here (a thread
        // already in another apartment keeps it, and COM still works there).
        let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let read = category(adapter).and_then(policy);
        if init.is_ok() {
            // SAFETY: balances the initialisation above.
            unsafe { CoUninitialize() };
        }
        read
    }

    /// The category of the network on the adapter with this GUID, from Windows' network list.
    fn category(adapter: &str) -> Option<Profile> {
        let id = GUID::try_from(adapter.trim_matches(['{', '}'])).ok()?;
        // SAFETY: COM calls on interfaces COM handed back, each released as it goes.
        unsafe {
            let list: INetworkListManager =
                CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).ok()?;
            let connections = list.GetNetworkConnections().ok()?;
            loop {
                let mut one = [None];
                connections.Next(&mut one, None).ok()?;
                let connection = one[0].take()?;
                if connection.GetAdapterId().ok() == Some(id) {
                    let category = connection.GetNetwork().ok()?.GetCategory().ok()?;
                    return Some(match category {
                        NLM_NETWORK_CATEGORY_PRIVATE => Profile::Private,
                        NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED => Profile::Domain,
                        _ => Profile::Public,
                    });
                }
            }
        }
    }

    /// The firewall's settings for `profile`, and the inbound rules that name this program.
    fn policy(profile: Profile) -> Option<Firewall> {
        let program = std::env::current_exe().ok()?;
        let program = program.to_string_lossy();
        let kind = NET_FW_PROFILE_TYPE2(profile.bit());
        // SAFETY: COM calls on interfaces COM handed back, each released as it goes.
        unsafe {
            let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL).ok()?;
            Some(Firewall {
                profile,
                on: policy.get_FirewallEnabled(kind).ok()?.as_bool(),
                block_all: policy.get_BlockAllInboundTraffic(kind).ok()?.as_bool(),
                asks: !policy.get_NotificationsDisabled(kind).ok()?.as_bool(),
                managed: policy.LocalPolicyModifyState().ok()? != NET_FW_MODIFY_STATE_OK,
                rules: rules(&policy, &program)?,
            })
        }
    }

    /// The inbound rules whose program is `program` (the path, in any case).
    ///
    /// # Safety
    /// `policy` is a live firewall policy object on this thread's apartment.
    unsafe fn rules(policy: &INetFwPolicy2, program: &str) -> Option<Vec<Rule>> {
        // SAFETY: as the caller promises.
        let each: IEnumVARIANT = unsafe { policy.Rules().ok()?._NewEnum().ok()?.cast().ok()? };
        let mut found = Vec::new();
        loop {
            let mut one = [VARIANT::default()];
            let mut fetched = 0;
            // SAFETY: room for one, and a count to say whether it came.
            let _ = unsafe { each.Next(&mut one, &mut fetched) };
            if fetched == 0 {
                return Some(found);
            }
            if one[0].vt() != VT_DISPATCH {
                continue;
            }
            // SAFETY: a VT_DISPATCH variant holds an IDispatch, released when `one` is.
            let rule = unsafe { one[0].Anonymous.Anonymous.Anonymous.pdispVal.as_ref() }
                .and_then(|rule| rule.cast::<INetFwRule>().ok());
            let Some(rule) = rule else {
                continue;
            };
            // SAFETY: property reads on a live rule.
            unsafe {
                if rule.Direction().ok() != Some(NET_FW_RULE_DIR_IN)
                    || !rule
                        .ApplicationName()
                        .is_ok_and(|name| name.to_string().eq_ignore_ascii_case(program))
                {
                    continue;
                }
                found.push(Rule {
                    allow: rule.Action().ok() == Some(NET_FW_ACTION_ALLOW),
                    enabled: rule.Enabled().is_ok_and(|enabled| enabled.as_bool()),
                    profiles: rule.Profiles().unwrap_or(0),
                    protocol: rule.Protocol().unwrap_or(0),
                });
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::super::look;
        use super::*;

        /// ★ On a real Windows: the firewall of the network the listener would use reads back,
        /// its category the one Windows' network list gives (printed beside the reading,
        /// `--nocapture`, to compare with `Get-NetConnectionProfile`). CONTROL for the rule
        /// reader: Windows' own inbound rules name svchost.exe, which the firewall hands back with
        /// its path expanded ("C:\WINDOWS\system32\svchost.exe", measured), and they are found
        /// when asked for it in another case.
        #[test]
        fn the_firewall_of_this_computers_network_reads_back() {
            let look = look(None);
            let Some(choice) = look.chosen() else {
                eprintln!("skipped: no network the listener would use");
                return;
            };
            let firewall = read(&choice.id).expect("the firewall did not read");
            println!(
                "{} ({}): {firewall:?} says {:?}",
                choice.name,
                choice.id,
                super::super::says(&firewall)
            );
            // SAFETY: COM for this test's thread, as `read` does it.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                let policy: INetFwPolicy2 =
                    CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL).unwrap();
                let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
                let svchost = format!(r"{root}\SYSTEM32\SVCHOST.EXE");
                let found = rules(&policy, &svchost).unwrap();
                println!("{} inbound rules name {svchost}", found.len());
                assert!(
                    !found.is_empty(),
                    "the rule reader found nothing for {svchost}"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(allow: bool, profiles: i32, protocol: i32) -> Rule {
        Rule {
            allow,
            enabled: true,
            profiles,
            protocol,
        }
    }

    /// A firewall that is on, asks, and holds `rules`, on a network of category `profile`.
    fn on(profile: Profile, rules: Vec<Rule>) -> Firewall {
        Firewall {
            profile,
            on: true,
            block_all: false,
            asks: true,
            managed: false,
            rules,
        }
    }

    const PRIVATE: i32 = 2;
    const PUBLIC: i32 = 4;
    const ALL: i32 = 0x7fff_ffff;

    /// ★ Each condition and its sentence, in the order Windows applies them: off says nothing;
    /// blocking everything beats every rule; a block rule beats an allow (what "Cancel" leaves,
    /// for TCP and UDP); a policy decides over rules made here; an allow for this network says
    /// nothing; with no rule yet, Public, then asking or not. CONTROL in each row: the same
    /// firewall with the one condition removed says something else.
    #[test]
    fn the_firewall_says_what_stands_in_the_way() {
        let allowed = on(
            Profile::Private,
            vec![rule(true, PRIVATE, TCP), rule(true, PRIVATE, UDP)],
        );
        assert_eq!(says(&allowed), None, "allowed on Private");
        assert_eq!(
            says(&Firewall {
                on: false,
                ..on(Profile::Public, vec![])
            }),
            None,
            "off"
        );

        let all = Firewall {
            block_all: true,
            ..allowed.clone()
        };
        assert_eq!(says(&all), Some("blocksAll"));

        // "Cancel" at Windows' question: block rules for TCP and UDP, on the network it was asked on.
        let cancelled = on(
            Profile::Private,
            vec![rule(false, PRIVATE, TCP), rule(false, PRIVATE, UDP)],
        );
        assert_eq!(says(&cancelled), Some("blocked"));
        // A block wins over an allow, for any protocol the stream uses.
        let mut both = allowed.rules.clone();
        both.push(rule(false, PRIVATE, UDP));
        assert_eq!(says(&on(Profile::Private, both)), Some("blocked"));
        // CONTROL: a block for another network, or a disabled one, is not this network's.
        assert_eq!(
            says(&on(
                Profile::Private,
                vec![rule(false, PUBLIC, TCP), rule(true, ALL, ANY)]
            )),
            None
        );
        let disabled = Rule {
            enabled: false,
            ..rule(false, PRIVATE, TCP)
        };
        assert_eq!(
            says(&on(
                Profile::Private,
                vec![disabled, rule(true, PRIVATE, TCP)]
            )),
            None
        );

        let managed = Firewall {
            managed: true,
            ..allowed.clone()
        };
        assert_eq!(says(&managed), Some("managed"));

        // Allowed on Private only, on a network Windows calls Public.
        let public = on(
            Profile::Public,
            vec![rule(true, PRIVATE, TCP), rule(true, PRIVATE, UDP)],
        );
        assert_eq!(says(&public), Some("public"));
        // CONTROL: allowed on Public too, it says nothing.
        assert_eq!(says(&on(Profile::Public, vec![rule(true, ALL, ANY)])), None);

        // No rule yet: Windows asks, or blocks without asking.
        assert_eq!(says(&on(Profile::Private, vec![])), Some("ask"));
        assert_eq!(
            says(&Firewall {
                asks: false,
                ..on(Profile::Private, vec![])
            }),
            Some("silent")
        );
        assert_eq!(says(&on(Profile::Domain, vec![])), Some("ask"));
        // An allow for UDP alone does not let the listener's TCP in.
        assert_eq!(
            says(&on(Profile::Private, vec![rule(true, PRIVATE, UDP)])),
            Some("ask")
        );
    }

    /// Off Windows there is no firewall to read.
    #[cfg(not(windows))]
    #[test]
    fn off_windows_nothing_is_read() {
        assert_eq!(read("{00000000-0000-0000-0000-000000000000}"), None);
    }
}
