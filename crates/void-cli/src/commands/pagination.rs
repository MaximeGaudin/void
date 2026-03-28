use crate::output::PaginationMeta;

pub fn parse_page(size: i64, page: i64) -> anyhow::Result<i64> {
    if size <= 0 {
        anyhow::bail!("--size must be greater than 0");
    }
    if page <= 0 {
        anyhow::bail!("--page must be greater than 0");
    }
    (page - 1)
        .checked_mul(size)
        .ok_or_else(|| anyhow::anyhow!("pagination overflow for page={page} size={size}"))
}

pub fn build_meta(current_page: i64, page_size: i64, total_elements: i64) -> PaginationMeta {
    let total_pages = if total_elements <= 0 {
        0
    } else {
        (total_elements + page_size - 1) / page_size
    };
    PaginationMeta {
        current_page,
        page_size,
        total_elements,
        total_pages,
    }
}
