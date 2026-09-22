// inventory: mom_e058_mc_undeclared
// `moment_calibration` row naming a symbol the file never declares. 7.1 refuses
// `Unknown symbol: zzz`; the editor names the block (E058).
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

moment_calibration;
zzz, y, [0, 1];
end;
