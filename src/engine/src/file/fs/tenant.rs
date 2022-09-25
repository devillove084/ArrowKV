// Copyright 2022 The template Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::path::PathBuf;

use tokio::fs;

use super::{bucket::FileSystemBucket, list::DirLister};
use crate::{
    error::{Error, Result},
    file::store_trait::{Bucket, Lister, Tenant},
};

pub struct FileSystemTenant {
    path: PathBuf,
}

impl FileSystemTenant {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait::async_trait]
impl Tenant for FileSystemTenant {
    fn bucket(&self, name: &str) -> Box<dyn Bucket> {
        let path = self.path.join(name);
        Box::new(FileSystemBucket::new(path))
    }

    async fn list_buckets(&self) -> Result<Box<dyn Lister<Item = String>>> {
        let dir = fs::read_dir(&self.path).await?;
        Ok(Box::new(DirLister::new(dir)))
    }

    async fn create_bucket(&self, name: &str) -> Result<Box<dyn Bucket>> {
        let path = self.path.join(name);
        if path.exists() {
            return Err(Error::AlreadyExists(format!("bucket {}", name)));
        }
        fs::create_dir_all(&path).await?;
        Ok(self.bucket(name))
    }

    async fn delete_bucket(&self, name: &str) -> Result<()> {
        let path = self.path.join(name);
        fs::remove_dir_all(&path).await?;
        Ok(())
    }
}

struct FileSystemBucketLister {
    dir: fs::ReadDir,
}

#[async_trait::async_trait]
impl Lister for FileSystemBucketLister {
    type Item = String;

    async fn next(&mut self, n: usize) -> Result<Vec<Self::Item>> {
        let mut result = Vec::new();
        for _i in 0..n {
            if let Some(ent) = self.dir.next_entry().await? {
                let file_name = ent
                    .file_name()
                    .into_string()
                    .map_err(|s| Error::Corrupted(format!("invalid name {:?}", s)))?;
                result.push(file_name);
            } else {
                break;
            }
        }
        Ok(result)
    }
}
