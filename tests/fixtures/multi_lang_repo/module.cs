// C# file in multi-language repository

namespace MultiLang
{
    public class CSharpClass
    {
        public int Value { get; set; }

        public CSharpClass(int value)
        {
            Value = value;
        }

        public int GetValue()
        {
            return Value;
        }
    }

    public static class CSharpHelper
    {
        public static int CSharpFunction(int x)
        {
            return x * 2;
        }
    }
}
