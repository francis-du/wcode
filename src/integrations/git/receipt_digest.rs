use super::{CheckAnnotation, CheckObservation, CheckReceiptFields};
use sha2::{Digest, Sha256};

fn hash_text(hasher: &mut Sha256, value: &str) {
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

fn hash_optional_text(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1u8]);
            hash_text(hasher, value);
        }
        None => hasher.update([0u8]),
    }
}

fn hash_optional_u64(hasher: &mut Sha256, value: Option<u64>) {
    match value {
        Some(value) => {
            hasher.update([1u8]);
            hasher.update(value.to_le_bytes());
        }
        None => hasher.update([0u8]),
    }
}

/// Digest over every captured field, length-prefixed so no two field layouts
/// can collide. It evidences capture integrity only; it is not an authentication
/// of the producer.
pub(crate) fn check_receipt_digest(
    receipt: &CheckReceiptFields,
    check: &CheckObservation,
    annotations: &[CheckAnnotation],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(receipt.schema_version.to_le_bytes());
    hash_text(&mut hasher, &receipt.provider);
    hash_text(&mut hasher, &receipt.repository);
    hasher.update(receipt.repository_id.to_le_bytes());
    hasher.update(receipt.app_id.to_le_bytes());
    hasher.update(check.id.to_le_bytes());
    hash_text(&mut hasher, &check.head_sha);
    hash_text(&mut hasher, &check.name);
    hash_text(&mut hasher, &check.source_id);
    hash_text(&mut hasher, &check.external_id);
    hash_text(&mut hasher, &check.status);
    hash_optional_text(&mut hasher, check.conclusion.as_deref());
    hasher.update((annotations.len() as u64).to_le_bytes());
    for annotation in annotations {
        hash_text(&mut hasher, &annotation.path);
        hash_optional_u64(&mut hasher, annotation.start_line);
        hash_optional_u64(&mut hasher, annotation.end_line);
        hash_text(&mut hasher, &annotation.annotation_level);
        hash_text(&mut hasher, &annotation.title);
        hash_text(&mut hasher, &annotation.message);
    }
    format!("{:x}", hasher.finalize())
}
