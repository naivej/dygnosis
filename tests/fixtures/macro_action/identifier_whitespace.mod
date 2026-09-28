// inventory: replacement whitespace prevents accidental identifier gluing
@#define tail = " y"
@#define head = "x "
var x@{tail};
var @{head}z;
model;
x=0;
y=0;
z=0;
end;
