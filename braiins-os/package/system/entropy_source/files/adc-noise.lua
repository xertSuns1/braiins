#!/usr/bin/lua
-- Copyright (C) 2020  Braiins Systems s.r.o.
--
-- This file is part of Braiins Open-Source Initiative (BOSI).
--
-- BOSI is free software: you can redistribute it and/or modify
-- it under the terms of the GNU General Public License as published by
-- the Free Software Foundation, either version 3 of the License, or
-- (at your option) any later version.
--
-- This program is distributed in the hope that it will be useful,
-- but WITHOUT ANY WARRANTY; without even the implied warranty of
-- MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
-- GNU General Public License for more details.
--
-- You should have received a copy of the GNU General Public License
-- along with this program.  If not, see <https://www.gnu.org/licenses/>.
--
-- Please, keep in mind that we may also license BOSI or any part thereof
-- under a proprietary license. For more information on the terms and conditions
-- of such proprietary license or if you have any other questions, please
-- contact us at opensource@braiins.com.

ADC_PATH = '/sys/devices/soc0/amba/f8007100.adc/iio:device0/in_voltage0_vccint_raw'
CONSTANT_VALUE_LIMIT = 1024

function getbit()
	local f = io.open(ADC_PATH, 'r') or error('cannot open '..ADC_PATH)
	local n = tonumber(f:read('*a'))
	f:close()
	return n % 2
end

function getbit_debias()
	local count = 0
	while true do
		local a, b = getbit(), getbit()
		if a ~= b then
			return a
		end
		count = count + 1
		if count > CONSTANT_VALUE_LIMIT then
			error('ADC has constant value')
		end
	end
end

function getbyte()
	local n = 0
	for i = 1, 8 do
		n = n * 2 + getbit_debias()
	end
	return n
end

io.stdout:setvbuf('no')
while true do
	local ok, err = io.stdout:write(string.char(getbyte()))
	if not ok then break end
end
