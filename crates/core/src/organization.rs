//! Logical classification only: moving a course never changes its files or versions.
use crate::Db;
use anyhow::{ensure, Context, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetOrganization {
    pub asset_id: String,
    pub collection_id: Option<String>,
    pub favorite: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub collections: Vec<Collection>,
    pub entries: Vec<AssetOrganization>,
}

fn name(value: &str) -> Result<&str> {
    let value = value.trim();
    ensure!(
        !value.is_empty() && value.chars().count() <= 80 && !value.chars().any(char::is_control),
        "名称应为 1–80 字，不能包含换行或控制字符"
    );
    Ok(value)
}
impl Db {
    pub fn organization(&self) -> Result<Organization> {
        let mut c = self.connection()?;
        let tx = c.transaction()?;
        let collections = tx
            .prepare("SELECT id,name,parent_id FROM collections ORDER BY name_key,id")?
            .query_map([], |r| {
                Ok(Collection {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    parent_id: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let entries = tx
            .prepare(
                "SELECT asset_id,collection_id,favorite FROM asset_organization ORDER BY asset_id",
            )?
            .query_map([], |r| {
                Ok(AssetOrganization {
                    asset_id: r.get(0)?,
                    collection_id: r.get(1)?,
                    favorite: r.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(Organization {
            collections,
            entries,
        })
    }
    pub fn create_collection(&self, value: &str, parent_id: Option<&str>) -> Result<Collection> {
        let name = name(value)?;
        let mut c = self.connection()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut parent = parent_id.map(str::to_owned);
        let mut depth = 1;
        while let Some(id) = parent {
            ensure!(depth < 8, "文件夹最多支持 8 层（含知识库）");
            depth += 1;
            parent = tx
                .query_row("SELECT parent_id FROM collections WHERE id=?1", [id], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .optional()?
                .context("上级知识库或文件夹不存在")?;
        }
        let result = Collection {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            parent_id: parent_id.map(str::to_owned),
        };
        tx.execute(
            "INSERT INTO collections(id,name,name_key,parent_id) VALUES(?1,?2,?3,?4)",
            params![result.id, name, name.to_lowercase(), parent_id],
        )
        .context("同一级中名称已存在，不能重复建立")?;
        tx.commit()?;
        Ok(result)
    }
    pub fn rename_collection(&self, id: &str, value: &str) -> Result<()> {
        let name = name(value)?;
        let count = self
            .connection()?
            .execute(
                "UPDATE collections SET name=?1,name_key=?2 WHERE id=?3",
                params![name, name.to_lowercase(), id],
            )
            .context("同一级中名称已存在，不能使用此名称")?;
        ensure!(count == 1, "分类不存在，请刷新列表");
        Ok(())
    }
    pub fn delete_collection(&self, id: &str) -> Result<()> {
        let mut c = self.connection()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM collections WHERE parent_id=?1) OR EXISTS(SELECT 1 FROM asset_organization WHERE collection_id=?1)",[id],|r|r.get(0))?;
        ensure!(!used, "只能删除空分类，请先移出课程并删除空子分组");
        ensure!(
            tx.execute("DELETE FROM collections WHERE id=?1", [id])? == 1,
            "分类不存在，请刷新列表"
        );
        tx.commit()?;
        Ok(())
    }
    pub fn move_assets(&self, asset_ids: &[String], collection_id: Option<&str>) -> Result<()> {
        ensure!(
            !asset_ids.is_empty() && asset_ids.len() <= 10000,
            "请选择 1–10000 份课程进行移动"
        );
        let mut c = self.connection()?;
        let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(id) = collection_id {
            ensure!(
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM collections WHERE id=?1)",
                    [id],
                    |r| r.get::<_, bool>(0)
                )?,
                "目标分类不存在，请刷新列表"
            );
        }
        for id in asset_ids {
            ensure!(
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",
                    [id],
                    |r| r.get::<_, bool>(0)
                )?,
                "课程不存在，整批移动未执行"
            );
            tx.execute("INSERT INTO asset_organization(asset_id,collection_id) VALUES(?1,?2) ON CONFLICT(asset_id) DO UPDATE SET collection_id=excluded.collection_id",params![id,collection_id])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn set_favorite(&self, asset_id: &str, favorite: bool) -> Result<()> {
        let c = self.connection()?;
        let count=c.execute("INSERT INTO asset_organization(asset_id,favorite) SELECT id,?2 FROM assets WHERE id=?1 ON CONFLICT(asset_id) DO UPDATE SET favorite=excluded.favorite",params![asset_id,favorite])?;
        ensure!(count == 1, "课程不存在，请刷新列表");
        Ok(())
    }
}
