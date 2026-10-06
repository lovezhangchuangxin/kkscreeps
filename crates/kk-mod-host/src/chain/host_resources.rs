//! 资源库存宿主能力；默认拒绝，具体槽显式开放。

pub trait ResourcesHost {
    fn resource_read(
        &mut self,
        _id: &str,
        _resource: &str,
    ) -> Result<kk_mod_api::resources::ResourceView, String> {
        Err("本次槽没有资源读取能力".into())
    }
    fn resource_transfer(
        &mut self,
        _from: &str,
        _to: &str,
        _resource: &str,
        _amount: i64,
        _loss: i64,
    ) -> Result<kk_mod_api::resources::ResourceTransfer, String> {
        Err("本次槽没有资源转账能力".into())
    }
}
