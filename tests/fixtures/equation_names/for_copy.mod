var y;
@#define is = 1:2
model;
y = y(-1);
@#for i in is
y = y(-1);
@#endfor
end;
