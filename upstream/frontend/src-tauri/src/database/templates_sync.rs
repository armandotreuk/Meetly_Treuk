use crate::database::models::Template;
use crate::summary::templates::defaults;
use anyhow::Result;
use sqlx::SqlitePool;

pub async fn sync_builtin_templates(pool: &SqlitePool) -> Result<()> {
    // Get all builtin template IDs
    let builtin_ids = defaults::list_builtin_template_ids();

    for id in builtin_ids {
        // Get template name and description from defaults
        let name = defaults::get_builtin_template_name(id).unwrap_or(id);
        let description = defaults::get_builtin_template_description(id).unwrap_or("");

        // Check if already exists in DB
        let existing = sqlx::query_as::<_, Template>(
            "SELECT * FROM templates WHERE name = $1 AND is_builtin = 1",
        )
        .bind(name)
        .fetch_optional(pool)
        .await?;

        if let Some(template) = existing {
            // Update schema_json if changed
            let content = defaults::get_builtin_template(id).unwrap_or("");
            if template.schema_json != content {
                sqlx::query(
                    r#"
                    UPDATE templates
                    SET description = $1, schema_json = $2, updated_at = $3
                    WHERE id = $4
                    "#,
                )
                .bind(description)
                .bind(content)
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(template.id)
                .execute(pool)
                .await?;
            }
        } else {
            // Get the template content
            let content = defaults::get_builtin_template(id).unwrap_or("");

            // Insert into database
            sqlx::query(
                r#"
                INSERT INTO templates (name, description, schema_json, is_builtin, created_at, updated_at)
                VALUES ($1, $2, $3, 1, datetime('now'), datetime('now'))
                "#,
            )
            .bind(name)
            .bind(description)
            .bind(content)
            .execute(pool)
            .await?;
        }
    }

    Ok(())
}
