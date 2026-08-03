use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Template {
    pub id: i64,
    pub name: String,
    pub description: String,
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
            // Check if builtin template with this name already exists
            let existing = sqlx::query_as::<_, Template>(
                "SELECT * FROM templates WHERE name = $1 AND is_builtin = 1",
            )
            .bind(name)
            .fetch_optional(pool)
            .await?;

            if let Some(template) = existing {
                // Update schema_json if changed
                if template.schema_json != schema_json {
                    sqlx::query(
                        r#"
                        UPDATE templates
                        SET description = $1, schema_json = $2, updated_at = $3
                        WHERE id = $4
                        "#,
                    )
                    .bind(description)
                    .bind(schema_json)
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
                    INSERT INTO templates (name, description, schema_json, is_builtin, created_at, updated_at)
                    VALUES ($1, $2, $3, 1, $4, $4)
                    "#,
                )
                .bind(name)
                .bind(description)
                .bind(schema_json)
                .bind(&now)
                .execute(pool)
                .await?;
            }
        }

        Ok(())
    }
}
