#include <rte_config.h>
#include <rte_common.h>
#include <rte_eal.h>
#include <rte_mempool.h>
#include <rte_mbuf.h>
#include <rte_ethdev.h>
#include <rte_byteorder.h>
#include <stdint.h>
#include <stddef.h>

/* Wrappers for DPDK inline functions so Rust can call them. */

struct rte_mbuf *dpdk_rte_pktmbuf_alloc(struct rte_mempool *mp) {
    return rte_pktmbuf_alloc(mp);
}

void dpdk_rte_pktmbuf_free(struct rte_mbuf *m) {
    rte_pktmbuf_free(m);
}

uint16_t dpdk_rte_pktmbuf_data_len(const struct rte_mbuf *m) {
    return rte_pktmbuf_data_len(m);
}

uint16_t dpdk_rte_pktmbuf_pkt_len(const struct rte_mbuf *m) {
    return rte_pktmbuf_pkt_len(m);
}

char *dpdk_rte_pktmbuf_append(struct rte_mbuf *m, uint16_t len) {
    return rte_pktmbuf_append(m, len);
}

void *dpdk_rte_pktmbuf_mtod(struct rte_mbuf *m) {
    return rte_pktmbuf_mtod(m, void *);
}

uint16_t dpdk_rte_eth_rx_burst(uint16_t port_id, uint16_t queue_id,
                               struct rte_mbuf **rx_pkts, const uint16_t nb_pkts) {
    return rte_eth_rx_burst(port_id, queue_id, rx_pkts, nb_pkts);
}

uint16_t dpdk_rte_eth_tx_burst(uint16_t port_id, uint16_t queue_id,
                               struct rte_mbuf **tx_pkts, const uint16_t nb_pkts) {
    return rte_eth_tx_burst(port_id, queue_id, tx_pkts, nb_pkts);
}

int32_t dpdk_rte_eth_tx_done_cleanup(uint16_t port_id, uint16_t queue_id,
                                     uint32_t free_cnt) {
    return rte_eth_tx_done_cleanup(port_id, queue_id, free_cnt);
}

void dpdk_rte_pktmbuf_refcnt_update(struct rte_mbuf *m, int16_t v) {
    rte_pktmbuf_refcnt_update(m, v);
}

uint16_t dpdk_rte_pktmbuf_refcnt_count(const struct rte_mbuf *m) {
    return rte_pktmbuf_refcnt_count(m);
}

unsigned int dpdk_rte_mempool_avail_count(const struct rte_mempool *mp) {
    return rte_mempool_avail_count(mp);
}

void dpdk_rte_mempool_cache_flush(struct rte_mempool *mp, unsigned lcore_id) {
    rte_mempool_cache_flush(mp, lcore_id);
}

void dpdk_rte_pktmbuf_adj(struct rte_mbuf *m, uint16_t len) {
    rte_pktmbuf_adj(m, len);
}

void dpdk_rte_pktmbuf_trim(struct rte_mbuf *m, uint16_t len) {
    rte_pktmbuf_trim(m, len);
}

uint16_t dpdk_rte_pktmbuf_headroom(const struct rte_mbuf *m) {
    return rte_pktmbuf_headroom(m);
}

uint16_t dpdk_rte_pktmbuf_tailroom(const struct rte_mbuf *m) {
    return rte_pktmbuf_tailroom(m);
}
