use std::fmt;

use super::*;

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct WasmSystem {
    pub args: Vec<Arg>,
    pub desc: String,
    pub name: String,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct Arg {
    pub name: String,
    pub param: SystemParam,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, PartialOrd, Ord)]
pub enum SystemParam {
    Commands,
    Query,
}

impl fmt::Display for SystemParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Commands => "commands",
            Self::Query => "query",
        })
    }
}

impl bindings::HostSystem for Host {
    fn new(&mut self, name: String) -> Result<Resource<WasmSystem>> {
        let system = self.table.push(WasmSystem {
            name,
            desc: String::new(),
            args: Vec::new(),
        })?;

        Ok(system)
    }

    fn add_commands(&mut self, system: Resource<WasmSystem>) -> Result<()> {
        add_param(self, system, SystemParam::Commands)
    }

    fn add_query(
        &mut self,
        system: Resource<WasmSystem>,
        _: Vec<bindings::QueryFor>,
    ) -> Result<()> {
        add_param(self, system, SystemParam::Query)
    }

    fn after(&mut self, _: Resource<WasmSystem>, _: Resource<WasmSystem>) -> Result<()> {
        Ok(())
    }

    fn before(&mut self, _: Resource<WasmSystem>, _: Resource<WasmSystem>) -> Result<()> {
        Ok(())
    }

    fn drop(&mut self, _: Resource<WasmSystem>) -> Result<()> {
        Ok(())
    }
}

fn add_param(host: &mut Host, system: Resource<WasmSystem>, param: SystemParam) -> Result<()> {
    let system = host.table.get_mut(&system)?;
    let count = system.args.len();
    system.args.push(Arg {
        name: format!("a{count}"),
        param,
    });

    Ok(())
}
