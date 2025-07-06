use anyhow::Result;
use sea_orm::{
    ActiveValue::{Set, Unchanged},
    ColumnTrait, Condition, ConnectionTrait, DatabaseBackend, EntityTrait, IntoActiveModel,
    JoinType, QueryFilter, QuerySelect, QueryTrait, RelationTrait, Select, Statement,
    sea_query::OnConflict,
};

use super::Db;
use crate::{
    entity::{media, media_set, media_up, set, up},
    state::MediaState,
};

fn select_active_medias() -> Select<media::Entity> {
    let active_media_from_up_subquery = media_up::Entity::find()
        .select_only()
        .column(media_up::Column::Id)
        .join(JoinType::InnerJoin, media_up::Relation::Up.def())
        .filter(up::Column::State.eq("Active"))
        .into_query();

    let active_media_from_set_subquery = media_set::Entity::find()
        .select_only()
        .column(media_set::Column::Id)
        .join(JoinType::InnerJoin, media_set::Relation::Set.def())
        .filter(set::Column::State.eq("Active"))
        .into_query();

    media::Entity::find().filter(
        Condition::any()
            .add(media::Column::Id.in_subquery(active_media_from_up_subquery))
            .add(media::Column::Id.in_subquery(active_media_from_set_subquery)),
    )
}

impl Db {
    pub async fn upsert_medias(
        &self,
        medias: impl IntoIterator<Item = media::Model>,
    ) -> Result<()> {
        media::Entity::insert_many(medias.into_iter().map(|m| m.into_active_model()))
            .on_conflict(
                OnConflict::column(media::Column::BvId)
                    .update_columns([media::Column::Title, media::Column::Id, media::Column::Type])
                    .to_owned(),
            )
            .exec_without_returning(&self.db)
            .await?;
        Ok(())
    }

    pub async fn set_media_state(&self, id: i64, state: MediaState) -> Result<()> {
        media::Entity::update(media::ActiveModel {
            id: Unchanged(id),
            state: Set(state.to_string()),
            ..Default::default()
        })
        .exec(&self.db)
        .await?;
        Ok(())
    }

    pub async fn all_medias(&self) -> Result<Vec<media::Model>> {
        media::Entity::find()
            .all(&self.db)
            .await
            .map_err(Into::into)
    }

    pub async fn all_active_medias(&self) -> Result<Vec<media::Model>> {
        select_active_medias()
            .all(&self.db)
            .await
            .map_err(Into::into)
    }

    pub async fn all_active_pending_medias(&self) -> Result<Vec<(media::Model, Vec<up::Model>)>> {
        select_active_medias()
            .filter(media::Column::State.eq("Pending"))
            .find_with_related(up::Entity)
            .all(&self.db)
            .await
            .map_err(Into::into)
    }

    /// Cleanup the medias whose up and set both are inactive/null
    pub async fn prune_medias(&self) -> Result<()> {
        self.db
            .execute(Statement::from_string(
                DatabaseBackend::Sqlite,
                r#"
DELETE FROM media
WHERE id IN (
    SELECT m.id
    FROM media m
    WHERE NOT EXISTS (
        SELECT 1 FROM media_up mu
        JOIN up u ON mu.up_id = u.up_id
        WHERE mu.id = m.id AND u.state != 'Inactive'
    )
    AND NOT EXISTS (
        SELECT 1 FROM media_set ms
        JOIN "set" s ON ms.set_id = s.set_id
        WHERE ms.id = m.id AND s.state != 'Inactive'
    )
);
"#,
            ))
            .await?;
        Ok(())
    }
}

