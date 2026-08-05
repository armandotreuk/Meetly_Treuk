use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

fn serialize_database_id_as_string<S>(id: &i64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&format!("db:{}", id))
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Template {
    #[serde(serialize_with = "serialize_database_id_as_string")]
    pub id: i64,
    pub name: String,
    pub description: String,
    pub stable_id: Option<String>,
    pub schema_json: String,
    pub is_builtin: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTemplateRequest {
    pub name: String,
    pub description: String,
    pub schema_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTemplateRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub schema_json: Option<String>,
}

pub struct TemplatesRepository;

impl TemplatesRepository {
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<Template>> {
        let templates = sqlx::query_as::<_, Template>(
            "SELECT * FROM templates ORDER BY is_builtin DESC, name ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(templates)
    }

    pub async fn list_user_templates(pool: &SqlitePool) -> Result<Vec<Template>> {
        let templates = sqlx::query_as::<_, Template>(
            "SELECT * FROM templates WHERE is_builtin = 0 ORDER BY name ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(templates)
    }

    pub async fn list_builtin_templates(pool: &SqlitePool) -> Result<Vec<Template>> {
        let templates = sqlx::query_as::<_, Template>(
            "SELECT * FROM templates WHERE is_builtin = 1 ORDER BY name ASC",
        )
        .fetch_all(pool)
        .await?;
        Ok(templates)
    }

    pub async fn get_by_id(pool: &SqlitePool, id: i64) -> Result<Option<Template>> {
        let template = sqlx::query_as::<_, Template>("SELECT * FROM templates WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?;
        Ok(template)
    }

    pub async fn create(pool: &SqlitePool, req: CreateTemplateRequest) -> Result<Template> {
        let now = Utc::now().to_rfc3339();

        let id = sqlx::query(
            r#"
            INSERT INTO templates (name, description, schema_json, is_builtin, created_at, updated_at)
            VALUES ($1, $2, $3, 0, $4, $4)
            "#,
        )
        .bind(&req.name)
        .bind(&req.description)
        .bind(&req.schema_json)
        .bind(&now)
        .execute(pool)
        .await?
        .last_insert_rowid();

        let template = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve created template"))?;

        Ok(template)
    }

    pub async fn update(
        pool: &SqlitePool,
        id: i64,
        req: UpdateTemplateRequest,
    ) -> Result<Template> {
        // First check if template exists and is not builtin
        let existing = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Template not found"))?;

        if existing.is_builtin != 0 {
            return Err(anyhow::anyhow!("Cannot modify built-in template"));
        }

        let now = Utc::now().to_rfc3339();

        let name = req.name.unwrap_or(existing.name);
        let description = req.description.unwrap_or(existing.description);
        let schema_json = req.schema_json.unwrap_or(existing.schema_json);

        sqlx::query(
            r#"
            UPDATE templates
            SET name = $1, description = $2, schema_json = $3, updated_at = $4
            WHERE id = $5
            "#,
        )
        .bind(&name)
        .bind(&description)
        .bind(&schema_json)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

        let template = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to retrieve updated template"))?;

        Ok(template)
    }

    pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
        // First check if template exists and is not builtin
        let existing = Self::get_by_id(pool, id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Template not found"))?;

        if existing.is_builtin != 0 {
            return Err(anyhow::anyhow!("Cannot delete built-in template"));
        }

        sqlx::query("DELETE FROM templates WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// Sync built-in templates from bundled resources to database
    pub async fn sync_builtin_templates(
        pool: &SqlitePool,
        templates: Vec<(&str, &str, &str)>,
    ) -> Result<()> {
        // templates: Vec<(name, description, schema_json)>
        for (name, description, schema_json) in templates {
            let stable_id = crate::summary::templates::defaults::list_builtin_template_ids()
                .into_iter()
                .find(|id| {
                    crate::summary::templates::defaults::get_builtin_template_name(id) == Some(name)
                });
            let existing = if let Some(stable_id) = stable_id {
                sqlx::query_as::<_, Template>(
                    "SELECT * FROM templates WHERE stable_id = $1 AND is_builtin = 1",
                )
                .bind(stable_id)
                .fetch_optional(pool)
                .await?
            } else {
                sqlx::query_as::<_, Template>(
                    "SELECT * FROM templates WHERE name = $1 AND is_builtin = 1 AND stable_id IS NULL",
                )
                .bind(name)
                .fetch_optional(pool)
                .await?
            };

            if let Some(template) = existing {
                // Update schema_json if changed
                if template.schema_json != schema_json
                    || template.name != name
                    || template.stable_id.as_deref() != stable_id
                {
                    sqlx::query(
                        r#"
                        UPDATE templates
                        SET name = $1, description = $2, schema_json = $3, stable_id = $4, updated_at = $5
                        WHERE id = $6
                        "#,
                    )
                    .bind(name)
                    .bind(description)
                    .bind(schema_json)
                    .bind(stable_id)
                    .bind(Utc::now().to_rfc3339())
                    .bind(template.id)
                    .execute(pool)
                    .await?;
                }
            } else {
                // Insert new builtin template
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    r#"
                    INSERT INTO templates (name, description, stable_id, schema_json, is_builtin, created_at, updated_at)
                    VALUES ($1, $2, $3, $4, 1, $5, $5)
                    "#,
                )
                .bind(name)
                .bind(description)
                .bind(stable_id)
                .bind(schema_json)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Template;

    #[test]
    fn repository_template_id_serializes_as_string() {
        let template = Template {
            id: 42,
            name: "Custom".to_string(),
            description: "Description".to_string(),
            stable_id: None,
            schema_json: "{}".to_string(),
            is_builtin: 0,
            created_at: "now".to_string(),
            updated_at: "now".to_string(),
        };

        let json = serde_json::to_value(template).expect("serialize template");
        assert_eq!(json["id"], "db:42");
    }
}
