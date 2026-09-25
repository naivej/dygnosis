var y (long_name='output');
parameters a (long_name='scale');
model;
[name='power'] y = -1 + 0 + a*y(-1)^2;
end;
