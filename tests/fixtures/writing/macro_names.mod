@#define is = 1:3
@#for i in is
var z;
@#endfor
@#define a = x1
@#define b = x2
var @{a};
var @{b};
var y (long_name='output');
model;
[name='a'] y = y(-1);
end;
