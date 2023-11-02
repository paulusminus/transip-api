use hickory_resolver::{
    config::{LookupIpStrategy, NameServerConfigGroup, ResolverConfig, ResolverOpts},
    Resolver,
};
use std::{convert::identity, net::IpAddr, thread::sleep, time::Duration};

use crate::error::Error;
pub use resolver::ResolverType;

mod error;
mod resolver;

pub type Result<T> = std::result::Result<T, Error>;

const MAX_RETRIES: usize = 720;
const WAIT_SECONDS: u64 = 5;

fn ipv6_resolver(
    group: NameServerConfigGroup,
    recursion: bool,
    ipv6_only: bool,
) -> Result<Resolver> {
    let config = ResolverConfig::from_parts(None, vec![], group);
    let mut options = ResolverOpts::default();
    if ipv6_only {
        options.ip_strategy = LookupIpStrategy::Ipv6Only;
    }
    options.recursion_desired = recursion;
    options.use_hosts_file = false;
    Resolver::new(config, options).map_err(Error::from)
}

fn recursive_resolver(ips: &[IpAddr], ipv6_only: bool) -> Result<Resolver> {
    let group = NameServerConfigGroup::from_ips_clear(ips, 53, false);
    ipv6_resolver(group, true, ipv6_only)
}

pub fn has_acme_challenge<S>(domain_name: S, challenge: S) -> Result<()>
where
    S: AsRef<str>,
{
    let resolvers = ResolverType::Google
        .recursive_resolver(true)
        .and_then(|resolver| resolver.authoritive_resolvers(domain_name.as_ref()))?;

    let mut i: usize = 0;

    sleep(Duration::from_secs(1));
    while !resolvers
        .iter()
        .map(|resolver| resolver.has_single_acme(domain_name.as_ref(), challenge.as_ref()))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .all(identity)
        && i < MAX_RETRIES
    {
        i += 1;
        tracing::warn!("Attempt {} failed", i);
        sleep(Duration::from_secs(WAIT_SECONDS));
    }
    if i >= MAX_RETRIES {
        tracing::error!("Timeout checking acme challenge record");
        Err(Error::AcmeChallege)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Display, net::IpAddr};

    use hickory_resolver::{
        lookup::{Ipv6Lookup, NsLookup},
        proto::rr::rdata::{AAAA, NS},
        Resolver,
    };

    use crate::{error::Error, ResolverType};

    fn to_string<D: Display>(d: D) -> String {
        d.to_string()
    }

    fn aaaa_to_ipv6(aaaa: AAAA) -> IpAddr {
        IpAddr::V6((*aaaa).clone())
    }

    fn lookup(name: &str) -> impl Fn(Resolver) -> Result<Ipv6Lookup, Error> + '_ {
        move |resolver| resolver.ipv6_lookup(name).map_err(Error::from)
    }

    fn ns_lookup(name: &str) -> impl Fn(Resolver) -> Result<NsLookup, Error> + '_ {
        move |resolver| resolver.ns_lookup(name).map_err(Error::from)
    }

    fn aaaa_mapper(f: fn(AAAA) -> IpAddr) -> impl Fn(Ipv6Lookup) -> Vec<IpAddr> {
        move |lookup| lookup.into_iter().map(f).collect()
    }

    fn ns_mapper(f: fn(NS) -> String) -> impl Fn(NsLookup) -> Vec<String> {
        move |lookup| lookup.into_iter().map(f).collect()
    }

    fn ipv6_address_lookup(name: &str) -> Result<Vec<IpAddr>, Error> {
        ResolverType::Google
            .resolver(true)
            .and_then(lookup(name))
            .map(aaaa_mapper(aaaa_to_ipv6))
    }

    fn nameservers_lookup(name: &str) -> Result<Vec<String>, Error> {
        ResolverType::Google
            .resolver(true)
            .and_then(ns_lookup(name))
            .map(ns_mapper(to_string))
    }

    #[test]
    fn test_www_paulmin_nl() {
        assert_eq!(
            ipv6_address_lookup("www.paulmin.nl.").unwrap(),
            vec!["2a01:7c8:bb0d:1bf:5054:ff:fedc:a36b"
                .parse::<IpAddr>()
                .unwrap(),],
        );
    }

    #[test]
    fn test_ns0_transip_net() {
        assert_eq!(
            ipv6_address_lookup("ns0.transip.net").unwrap(),
            vec!["2a01:7c8:dddd:195::195".parse::<IpAddr>().unwrap(),],
        );
    }

    #[test]
    fn test_ns1_transip_nl() {
        assert_eq!(
            ipv6_address_lookup("ns1.transip.nl.").unwrap(),
            vec!["2a01:7c8:7000:195::195".parse::<IpAddr>().unwrap(),],
        );
    }

    #[test]
    fn test_ns2_transip_eu() {
        assert_eq!(
            ipv6_address_lookup("ns2.transip.eu.").unwrap(),
            vec!["2a01:7c8:f:c1f::195".parse::<IpAddr>().unwrap(),],
        );
    }

    #[test]
    fn test_domain_ns() {
        let mut domain = nameservers_lookup("paulmin.nl").unwrap();
        domain.sort();
        assert_eq!(
            domain,
            vec![
                "ns0.transip.net.".to_owned(),
                "ns1.transip.nl.".to_owned(),
                "ns2.transip.eu.".to_owned(),
            ],
        );
    }
}
