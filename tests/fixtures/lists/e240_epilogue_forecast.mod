// inventory: lists_e240_epilogue_forecast
// `gg` is born in the epilogue block, so 7.1 knows it and reports its type.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

epilogue;
gg = y + c;
end;

forecast gg;
