// Minimal, single-packet NFQUEUE verdict helper for the isolated NFQUEUE test.
// It is intentionally not production daemon code.
#include <arpa/inet.h>
#include <errno.h>
#include <linux/netfilter.h>
#include <libnetfilter_queue/libnetfilter_queue.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

struct configuration {
  uint32_t verdict;
  unsigned int handled;
};

static int handle_packet(struct nfq_q_handle *queue, struct nfgenmsg *message,
                         struct nfq_data *data, void *opaque) {
  (void)message;
  struct configuration *configuration = opaque;
  struct nfqnl_msg_packet_hdr *header = nfq_get_msg_packet_hdr(data);
  if (header == NULL) return 0;
  const uint32_t id = ntohl(header->packet_id);
  configuration->handled++;
  return nfq_set_verdict(queue, id, configuration->verdict, 0, NULL);
}

int main(int argc, char **argv) {
  if (argc != 2 || (strcmp(argv[1], "allow") != 0 &&
                    strcmp(argv[1], "deny") != 0)) {
    fprintf(stderr, "usage: %s <allow|deny>\n", argv[0]);
    return 64;
  }
  struct configuration configuration = {
      .verdict = strcmp(argv[1], "allow") == 0 ? NF_ACCEPT : NF_DROP,
      .handled = 0,
  };
  struct nfq_handle *handle = nfq_open();
  if (handle == NULL) {
    perror("nfq_open");
    return 1;
  }
  (void)nfq_unbind_pf(handle, AF_INET);
  if (nfq_bind_pf(handle, AF_INET) < 0) {
    perror("nfq_bind_pf");
    nfq_close(handle);
    return 1;
  }
  struct nfq_q_handle *queue =
      nfq_create_queue(handle, 4242, handle_packet, &configuration);
  if (queue == NULL) {
    perror("nfq_create_queue");
    nfq_close(handle);
    return 1;
  }
  if (nfq_set_mode(queue, NFQNL_COPY_PACKET, 0xffff) < 0) {
    perror("nfq_set_mode");
    nfq_destroy_queue(queue);
    nfq_close(handle);
    return 1;
  }
  puts("ready");
  fflush(stdout);
  const int fd = nfq_fd(handle);
  char buffer[8192] __attribute__((aligned));
  while (configuration.handled == 0) {
    const ssize_t received = recv(fd, buffer, sizeof(buffer), 0);
    if (received < 0 && errno == EINTR) continue;
    if (received < 0) {
      perror("recv");
      nfq_destroy_queue(queue);
      nfq_close(handle);
      return 1;
    }
    if (nfq_handle_packet(handle, buffer, (int)received) < 0) {
      perror("nfq_handle_packet");
      nfq_destroy_queue(queue);
      nfq_close(handle);
      return 1;
    }
  }
  nfq_destroy_queue(queue);
  nfq_close(handle);
  return 0;
}

