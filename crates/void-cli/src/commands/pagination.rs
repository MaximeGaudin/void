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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_page_first_page_zero_offset() {
        assert_eq!(parse_page(10, 1).unwrap(), 0);
    }

    #[test]
    fn parse_page_second_page_offset() {
        assert_eq!(parse_page(10, 2).unwrap(), 10);
    }

    #[test]
    fn parse_page_rejects_non_positive_size() {
        let err = parse_page(0, 1).unwrap_err();
        assert!(err.to_string().contains("size"));
        let err = parse_page(-5, 1).unwrap_err();
        assert!(err.to_string().contains("size"));
    }

    #[test]
    fn parse_page_rejects_non_positive_page() {
        let err = parse_page(10, 0).unwrap_err();
        assert!(err.to_string().contains("page"));
        let err = parse_page(10, -1).unwrap_err();
        assert!(err.to_string().contains("page"));
    }

    #[test]
    fn parse_page_overflow() {
        let err = parse_page(i64::MAX, 3).unwrap_err();
        assert!(err.to_string().contains("overflow"), "{err}");
    }

    #[test]
    fn build_meta_zero_total() {
        let m = build_meta(1, 50, 0);
        assert_eq!(m.total_pages, 0);
        assert_eq!(m.total_elements, 0);
    }

    #[test]
    fn build_meta_negative_total_treated_as_empty() {
        let m = build_meta(1, 50, -3);
        assert_eq!(m.total_pages, 0);
    }

    #[test]
    fn build_meta_exact_page_boundary() {
        let m = build_meta(2, 10, 100);
        assert_eq!(m.total_pages, 10);
    }

    #[test]
    fn build_meta_partial_last_page() {
        let m = build_meta(1, 10, 95);
        assert_eq!(m.total_pages, 10);
    }
}
