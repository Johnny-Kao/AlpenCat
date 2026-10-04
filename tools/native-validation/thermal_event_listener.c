#define _GNU_SOURCE
#include <errno.h>
#include <linux/genetlink.h>
#include <linux/netlink.h>
#include <linux/thermal.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#ifndef NLA_ALIGNTO
#define NLA_ALIGNTO 4
#endif
#ifndef NLA_ALIGN
#define NLA_ALIGN(len) (((len) + NLA_ALIGNTO - 1) & ~(NLA_ALIGNTO - 1))
#endif
#ifndef NLA_HDRLEN
#define NLA_HDRLEN ((int)NLA_ALIGN(sizeof(struct nlattr)))
#endif
#ifndef THERMAL_GENL_FAMILY_NAME
#define THERMAL_GENL_FAMILY_NAME "thermal"
#endif
#ifndef THERMAL_GENL_EVENT_GROUP_NAME
#define THERMAL_GENL_EVENT_GROUP_NAME "event"
#endif

static int add_attr(struct nlmsghdr *nlh, size_t maxlen, int type,
                    const void *data, size_t len) {
    size_t attr_len = NLA_HDRLEN + len;
    size_t newlen = NLMSG_ALIGN(nlh->nlmsg_len) + NLA_ALIGN(attr_len);
    if (newlen > maxlen)
        return -1;

    struct nlattr *nla =
        (struct nlattr *)((char *)nlh + NLMSG_ALIGN(nlh->nlmsg_len));
    nla->nla_type = type;
    nla->nla_len = attr_len;
    memcpy((char *)nla + NLA_HDRLEN, data, len);
    memset((char *)nla + attr_len, 0, NLA_ALIGN(attr_len) - attr_len);
    nlh->nlmsg_len = newlen;
    return 0;
}

static int resolve_thermal(int fd, uint16_t *family_id, uint32_t *group_id) {
    char buf[8192] = {0};
    struct nlmsghdr *nlh = (struct nlmsghdr *)buf;
    struct genlmsghdr *genl =
        (struct genlmsghdr *)(buf + NLMSG_HDRLEN);

    nlh->nlmsg_len = NLMSG_HDRLEN + GENL_HDRLEN;
    nlh->nlmsg_type = GENL_ID_CTRL;
    nlh->nlmsg_flags = NLM_F_REQUEST;
    nlh->nlmsg_seq = 1;
    genl->cmd = CTRL_CMD_GETFAMILY;
    genl->version = 1;

    const char name[] = THERMAL_GENL_FAMILY_NAME;
    if (add_attr(nlh, sizeof(buf), CTRL_ATTR_FAMILY_NAME,
                 name, sizeof(name)) < 0)
        return -1;

    struct sockaddr_nl nladdr = {.nl_family = AF_NETLINK};
    if (sendto(fd, buf, nlh->nlmsg_len, 0,
               (struct sockaddr *)&nladdr, sizeof(nladdr)) < 0)
        return -1;

    ssize_t nr = recv(fd, buf, sizeof(buf), 0);
    if (nr < 0)
        return -1;

    int remaining_nl = (int)nr;
    for (nlh = (struct nlmsghdr *)buf;
         NLMSG_OK(nlh, remaining_nl);
         nlh = NLMSG_NEXT(nlh, remaining_nl)) {
        if (nlh->nlmsg_type == NLMSG_ERROR)
            return -1;

        genl = (struct genlmsghdr *)NLMSG_DATA(nlh);
        int rem = (int)nlh->nlmsg_len - NLMSG_HDRLEN - GENL_HDRLEN;
        struct nlattr *attr =
            (struct nlattr *)((char *)genl + GENL_HDRLEN);

        for (; rem >= (int)sizeof(*attr) &&
               attr->nla_len >= sizeof(*attr) &&
               attr->nla_len <= rem;
             rem -= NLA_ALIGN(attr->nla_len),
             attr = (struct nlattr *)((char *)attr +
                                     NLA_ALIGN(attr->nla_len))) {
            int type = attr->nla_type & NLA_TYPE_MASK;
            void *payload = (char *)attr + NLA_HDRLEN;
            int plen = attr->nla_len - NLA_HDRLEN;

            if (type == CTRL_ATTR_FAMILY_ID && plen >= 2) {
                memcpy(family_id, payload, 2);
            } else if (type == CTRL_ATTR_MCAST_GROUPS) {
                int grem = plen;
                struct nlattr *grp = (struct nlattr *)payload;

                for (; grem >= (int)sizeof(*grp) &&
                       grp->nla_len >= sizeof(*grp) &&
                       grp->nla_len <= grem;
                     grem -= NLA_ALIGN(grp->nla_len),
                     grp = (struct nlattr *)((char *)grp +
                                            NLA_ALIGN(grp->nla_len))) {
                    const char *gname = NULL;
                    uint32_t gid = 0;
                    int irem = grp->nla_len - NLA_HDRLEN;
                    struct nlattr *ga =
                        (struct nlattr *)((char *)grp + NLA_HDRLEN);

                    for (; irem >= (int)sizeof(*ga) &&
                           ga->nla_len >= sizeof(*ga) &&
                           ga->nla_len <= irem;
                         irem -= NLA_ALIGN(ga->nla_len),
                         ga = (struct nlattr *)((char *)ga +
                                              NLA_ALIGN(ga->nla_len))) {
                        int gt = ga->nla_type & NLA_TYPE_MASK;
                        void *gp = (char *)ga + NLA_HDRLEN;
                        int gplen = ga->nla_len - NLA_HDRLEN;

                        if (gt == CTRL_ATTR_MCAST_GRP_NAME &&
                            gplen > 0)
                            gname = (const char *)gp;

                        if (gt == CTRL_ATTR_MCAST_GRP_ID &&
                            gplen >= 4)
                            memcpy(&gid, gp, 4);
                    }

                    if (gname &&
                        strcmp(gname,
                               THERMAL_GENL_EVENT_GROUP_NAME) == 0)
                        *group_id = gid;
                }
            }
        }
    }

    return (*family_id && *group_id) ? 0 : -1;
}

static void iso_timestamp(char *out, size_t n) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);

    struct tm tm;
    gmtime_r(&ts.tv_sec, &tm);

    snprintf(out, n,
             "%04d-%02d-%02dT%02d:%02d:%02d.%09ldZ",
             tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday,
             tm.tm_hour, tm.tm_min, tm.tm_sec, ts.tv_nsec);
}

static void print_cap_event(struct genlmsghdr *genl, int attrlen) {
    char ts[64];
    iso_timestamp(ts, sizeof(ts));

    printf("{\"timestamp\":\"%s\","
           "\"event_cmd\":%u,"
           "\"cpu_capabilities\":[",
           ts, genl->cmd);

    int first = 1;
    struct nlattr *attr =
        (struct nlattr *)((char *)genl + GENL_HDRLEN);
    int rem = attrlen;

    for (; rem >= (int)sizeof(*attr) &&
           attr->nla_len >= sizeof(*attr) &&
           attr->nla_len <= rem;
         rem -= NLA_ALIGN(attr->nla_len),
         attr = (struct nlattr *)((char *)attr +
                                 NLA_ALIGN(attr->nla_len))) {
        int type = attr->nla_type & NLA_TYPE_MASK;
        if (type != THERMAL_GENL_ATTR_CPU_CAPABILITY)
            continue;

        int nrem = attr->nla_len - NLA_HDRLEN;
        struct nlattr *na =
            (struct nlattr *)((char *)attr + NLA_HDRLEN);

        uint32_t cpu = UINT32_MAX;
        uint32_t perf = UINT32_MAX;
        uint32_t eff = UINT32_MAX;

        for (; nrem >= (int)sizeof(*na) &&
               na->nla_len >= sizeof(*na) &&
               na->nla_len <= nrem;
             nrem -= NLA_ALIGN(na->nla_len),
             na = (struct nlattr *)((char *)na +
                                   NLA_ALIGN(na->nla_len))) {
            int nt = na->nla_type & NLA_TYPE_MASK;
            void *np = (char *)na + NLA_HDRLEN;
            int nplen = na->nla_len - NLA_HDRLEN;

            uint32_t value = 0;
            if (nplen < 4)
                continue;
            memcpy(&value, np, 4);

            if (nt == THERMAL_GENL_ATTR_CPU_CAPABILITY_ID) {
                cpu = value;
            } else if (nt ==
                       THERMAL_GENL_ATTR_CPU_CAPABILITY_PERFORMANCE) {
                perf = value;
            } else if (nt ==
                       THERMAL_GENL_ATTR_CPU_CAPABILITY_EFFICIENCY) {
                eff = value;
                if (!first)
                    printf(",");
                printf("{\"cpu\":%u,"
                       "\"performance\":%u,"
                       "\"efficiency\":%u}",
                       cpu, perf, eff);
                first = 0;
                cpu = perf = eff = UINT32_MAX;
            }
        }
    }

    printf("]}\n");
    fflush(stdout);
}

int main(int argc, char **argv) {
    int timeout_sec = 60;
    if (argc > 1)
        timeout_sec = atoi(argv[1]);
    if (timeout_sec <= 0)
        timeout_sec = 60;

    int fd = socket(AF_NETLINK, SOCK_RAW, NETLINK_GENERIC);
    if (fd < 0) {
        perror("socket");
        return 2;
    }

    struct sockaddr_nl local = {
        .nl_family = AF_NETLINK,
        .nl_pid = (uint32_t)getpid(),
    };

    if (bind(fd, (struct sockaddr *)&local, sizeof(local)) < 0) {
        perror("bind");
        close(fd);
        return 2;
    }

    uint16_t family = 0;
    uint32_t group = 0;

    if (resolve_thermal(fd, &family, &group) < 0) {
        fprintf(stderr,
                "thermal generic-netlink family/event group unavailable\n");
        close(fd);
        return 3;
    }

    if (setsockopt(fd, SOL_NETLINK, NETLINK_ADD_MEMBERSHIP,
                   &group, sizeof(group)) < 0) {
        perror("NETLINK_ADD_MEMBERSHIP");
        close(fd);
        return 4;
    }

    fprintf(stderr,
            "listening family=%u group=%u timeout=%ds\n",
            family, group, timeout_sec);

    time_t end = time(NULL) + timeout_sec;
    char buf[65536];

    while (time(NULL) < end) {
        int remain = (int)(end - time(NULL));
        struct pollfd pfd = {.fd = fd, .events = POLLIN};

        int pr = poll(&pfd, 1, remain * 1000);
        if (pr == 0)
            break;
        if (pr < 0) {
            if (errno == EINTR)
                continue;
            perror("poll");
            close(fd);
            return 5;
        }

        ssize_t nr = recv(fd, buf, sizeof(buf), 0);
        if (nr < 0) {
            if (errno == EINTR)
                continue;
            perror("recv");
            close(fd);
            return 5;
        }

        int len = (int)nr;
        for (struct nlmsghdr *nlh =
                 (struct nlmsghdr *)buf;
             NLMSG_OK(nlh, len);
             nlh = NLMSG_NEXT(nlh, len)) {
            if (nlh->nlmsg_type != family)
                continue;

            struct genlmsghdr *genl =
                (struct genlmsghdr *)NLMSG_DATA(nlh);
            int attrlen =
                (int)nlh->nlmsg_len -
                NLMSG_HDRLEN - GENL_HDRLEN;

            if (genl->cmd ==
                THERMAL_GENL_EVENT_CPU_CAPABILITY_CHANGE)
                print_cap_event(genl, attrlen);
        }
    }

    close(fd);
    return 0;
}
