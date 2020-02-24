# Copyright (C) 2019  Braiins Systems s.r.o.
#
# This file is part of Braiins Open-Source Initiative (BOSI).
#
# BOSI is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.
#
# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.
#
# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <https://www.gnu.org/licenses/>.
#
# Please, keep in mind that we may also license BOSI or any part thereof
# under a proprietary license. For more information on the terms and conditions
# of such proprietary license or if you have any other questions, please
# contact us at opensource@braiins.com.

include $(TOPDIR)/rules.mk
include $(INCLUDE_DIR)/kernel.mk

PKG_NAME:=bos_plus
PKG_RELEASE:=1

PKG_MAINTAINER:=Braiins <braiins@braiins.com>

BOS_FEEDS_SERVER:=https://feeds.braiins-os.org/$(SUBTARGET)
BOS_FEEDS_CONFIG:=/etc/opkg/bosfeeds.conf

include $(INCLUDE_DIR)/package.mk

define Package/bos_plus
  SECTION:=utils
  CATEGORY:=Utilities
  TITLE:=Upgrade to BOS+
endef

define Package/bos_plus/description
 Meta package for firmware upgrade to BOS+.
endef

define Build/Compile
endef

# LEDE requires some data for installation
define Package/bos_plus/install
	$(INSTALL_DIR) $(1)/etc
endef

define Package/bos_plus/preinst
#!/bin/sh

# if run within buildroot exit
[ -n "$${IPKG_INSTROOT}" ] && exit 0

# ensure compatibility with old firmwares which does not have bos_mode marker
# moreover, old firmware has package installed only in NAND mode
if [ -f "/lib/functions/bos-defaults.sh" ]; then
	. /lib/functions/bos-defaults.sh

	# do not write to NAND when package installation has been forced for incorrect mode
	[ $$BOS_MODE == 'nand' ] || exit 1
fi

PACKAGES_NAME="Packages"
PACKAGES_PATH="/tmp/$${PACKAGES_NAME}"
SYSUPGRADE_PATH="/tmp/sysupgrade.tar"
BOS_FEEDS="/etc/bosplusfeeds.conf"

if [ ! -f "$${BOS_FEEDS}" ]; then
	echo "Missing configuration file $${BOS_FEEDS}!" >&2
	exit 1
fi

# parse server address from opkg configuration file
feeds_server=$$(cat "$${BOS_FEEDS}")

if [ -z "$${feeds_server}" ]; then
	echo "Cannot find feeds server in $${BOS_FEEDS}!" >&2
	exit 1
fi

current_fw_version() {
	if ! cat /etc/bos_version 2>/dev/null; then
		awk '/Package: /{p=$$2} /Version: /{v=$$2} \
			 /Status: /{if (p == "firmware" && $$NF == "installed") print v}' \
			'/usr/lib/opkg/status'
	fi
}

latest_fw_version() {
	result=$$(awk '/Package: /{p=$$2} /Version: /{v=$$2} /Require: /{r=$$2} \
		/Filename: /{if (p == "firmware") print $$2" "r}' "$$1" | tail -n1)
	eval $$2=$${result% *}
	eval $$3=$${result#* }
}

required_fw_version() {
	result=$$(awk '/Package: /{p=$$2} /Version: /{v=$$2} /Require: /{r=$$2} \
		/Filename: /{if (p == "firmware" && v == "'$$2'") print $$2" "r}' "$$1")
	eval $$3=$${result% *}
	eval $$4=$${result#* }
}

get_firmware_name() {
	echo "Downloading package list..."
	wget -O "$${PACKAGES_PATH}.gz" "$${feeds_server}/$${PACKAGES_NAME}.gz" || return 1
	gzip -df "$${PACKAGES_PATH}.gz" || return 1
	wget -O "$${PACKAGES_PATH}.sig" "$${feeds_server}/$${PACKAGES_NAME}.sig" || return 1
	echo "Checking signature..."
	usign -V -m "$${PACKAGES_PATH}" -P "/etc/opkg/keys" || return 1

	local fw_version=$$(current_fw_version)
	local filename
	local require
	latest_fw_version "$${PACKAGES_PATH}" filename require
	if [ ! "$${fw_version}" "<" "$${require}" ]; then
		eval $$1=$${filename}
		return 0
	fi

	while [ "$${fw_version}" "<" "$${require}" ]; do
		required_fw_version "$${PACKAGES_PATH}" $${require} filename require
		[ -n "$${filename}" ] || return 1
	done
	echo "Using previous firmware to satisfy requirements!"
	eval $$1="$${filename%.ipk}.tar"
}

if get_firmware_name firmware_name; then
	echo "Running system upgrade..."
	wget -O "$${SYSUPGRADE_PATH}" "$${feeds_server}/$${firmware_name}" || exit 1
	sysupgrade "$${SYSUPGRADE_PATH}"
else
	echo "Cannot find required firmware on server!" >&2
	exit 1
fi

# the package is not actually installed
# wait to suppress error printing
sleep 10
exit 2
endef

$(eval $(call BuildPackage,bos_plus))
