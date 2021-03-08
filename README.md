# ArrowKV

> 本项目设计的目的是使用Rust设计一套现代化的KV存储引擎

## Features

* 只支持 PM (非易失性内存)
* 使用 Bw-Tree 优化并发索引
* 以 8 Bytes 区分大小对象的写入，保证原子性（利用 PM 自身特性）
* [WiscKey](https://www.usenix.org/system/files/conference/fast16/fast16-papers-lu.pdf)
* [Epoch based reclamation](https://docs.rs/crossbeam/0.8.0/crossbeam/epoch/index.html) 
* Some [LSA](http://www.vldb.org/pvldb/vol11/p458-merritt.pdf) Feature
* Work-stealing
* [Single-Level-LSM Tree](https://www.usenix.org/system/files/fast19-kaiyrakhmet.pdf)
* 优化 Level-0 层的 seach 操作，优先搜索最新的 sst，利用 sst 前缀压缩机制优化搜索速度

## Design

> 简化 LSM-Tree 中多层 Level 的设计，利用 PM 的特性省去写数据时的 WAL 层，采用单层 Level，进行 Major Compaction 后，直接对 Level-0 层中的数据进行选择性合并，落盘在 PM 或者 SSD 中的数据直接为全局有序 Block，再使用 Bw-Tree(下图以 B+Tree 代替)对落盘的数据进行索引，提高查询速度。

![Single-Level KV Store with Persistent Memory](https://pic4.zhimg.com/v2-84981aa8d374873f5df4ebf1f7be23ec_1440w.jpg?source=172ae18b) 
