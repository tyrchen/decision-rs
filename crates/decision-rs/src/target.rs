//! Immutable hosted selection and per-call model-dependent routing.

use crate::{
    CloudflareAccountId, Endpoint, Error, Model, Provider, account::validate_account,
    backend::Backend,
};

#[derive(Debug)]
pub(crate) struct HostedSelection {
    pub provider: Provider,
    pub account: Option<CloudflareAccountId>,
}
impl HostedSelection {
    pub fn into_target(self) -> Result<Target, Error> {
        validate_account(self.provider, self.account.as_ref())?;
        Ok(match self.provider {
            Provider::Cloudflare => Target::Cloudflare(
                self.account
                    .ok_or(Error::InvalidConfiguration("missing Cloudflare account ID"))?,
            ),
            Provider::OpenAi => Target::Fixed(Endpoint::openai_decisions(
                "https://api.openai.com/v1/decisions",
            )?),
            Provider::OpenRouter => Target::Fixed(Endpoint::jev_decisions(
                "https://openrouter.ai/api/alpha/decisions",
            )?),
            Provider::TypeSafe => Target::Fixed(Endpoint::system_one(
                "https://api.typesafe.ai/v1/systemone",
            )?),
        })
    }
}
#[derive(Debug)]
pub(crate) enum Target {
    Fixed(Endpoint),
    Cloudflare(CloudflareAccountId),
}
#[derive(Debug)]
pub(crate) struct ResolvedCall {
    pub model: Model,
    pub wire_model: String,
    pub routing_model: Option<String>,
    pub endpoint: Endpoint,
}
impl Target {
    pub fn resolve(
        &self,
        model: &Model,
        provider: Option<Provider>,
    ) -> Result<ResolvedCall, Error> {
        let (model, wire_model) = model.resolve(provider)?;
        let (endpoint, routing_model) = match self {
            Self::Fixed(endpoint) => (endpoint.clone(), None),
            Self::Cloudflare(account) => {
                let routing_model = match model {
                    Model::Clef => "@cf/cloudflare/clef",
                    Model::ClefFlash => "@cf/cloudflare/clef-flash",
                    Model::ClefOmni => "@cf/cloudflare/clef-omni",
                    _ => return Err(Error::UnsupportedModel),
                };
                let endpoint = Endpoint::new(
                    &format!(
                        "https://api.cloudflare.com/client/v4/accounts/{}/ai/run/{routing_model}",
                        account.as_str()
                    ),
                    Backend::Cloudflare,
                )?;
                (endpoint, Some(routing_model.to_owned()))
            }
        };
        Ok(ResolvedCall {
            model,
            wire_model,
            routing_model,
            endpoint,
        })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(Model::Default, "clef", Model::Clef)]
    #[case(Model::Clef, "clef", Model::Clef)]
    #[case(Model::ClefFlash, "clef-flash", Model::ClefFlash)]
    #[case(Model::ClefOmni, "clef-omni", Model::ClefOmni)]
    fn test_should_resolve_route_and_selector_atomically(
        #[case] model: Model,
        #[case] selector: &str,
        #[case] resolved: Model,
    ) {
        let account = "0123456789abcdef0123456789abcdef";
        let target = HostedSelection {
            provider: Provider::Cloudflare,
            account: Some(account.parse().unwrap()),
        }
        .into_target()
        .unwrap();
        let call = target.resolve(&model, Some(Provider::Cloudflare)).unwrap();
        assert_eq!(call.model, resolved);
        assert_eq!(call.wire_model, selector);
        assert_eq!(
            call.routing_model,
            Some(format!("@cf/cloudflare/{selector}"))
        );
        assert_eq!(call.endpoint.url.as_str(), format!("https://api.cloudflare.com/client/v4/accounts/{account}/ai/run/@cf/cloudflare/{selector}"));
        assert!(matches!(call.endpoint.backend, Backend::Cloudflare));
        assert!(!format!("{call:?} {target:?}").contains(account));
        assert_eq!(
            target
                .resolve(&Model::Default, Some(Provider::Cloudflare))
                .unwrap()
                .wire_model,
            "clef"
        );
    }
}
