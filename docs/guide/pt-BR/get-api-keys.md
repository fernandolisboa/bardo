---
id: get-api-keys
title: Como obter cada chave de API
group: getting-started
---

# Como obter cada chave de API

Como obter a chave de cada provedor de [Configurações › Chaves de API](bardo:go/settings/keys), uma seção por provedor. No Bardo, o **Passo a passo** de cada cartão mostra os mesmos passos numa tela própria, com o campo do cartão no fim para colar a chave. Veja [Suas chaves de API](api-keys.md) para o que cada provedor faz.

Algumas coisas valem para todo provedor:

- **Copie a chave assim que ela aparecer.** A maioria dos provedores mostra uma chave nova uma única vez; se você perder, crie outra e troque a antiga no Bardo.
- **Guarde só para você.** Cole a chave apenas no Bardo; o Bardo guarda no Gerenciador de Credenciais do Windows e esconde a chave em todo o resto.
- **Teste.** Depois de salvar, **Testar chave** faz a chamada mais barata que precisa da chave e diz o que está errado, se houver algo.

<a id="byteplus"></a>
## BytePlus ModelArk

Você só precisa disto para clipes interpretados com o Seedance 2.5 direto do BytePlus ModelArk, que faz um rascunho barato em 480p e depois completa esse mesmo rascunho em 1080p. O ModelArk só lê o vídeo de controle de uma tomada por um link, então o Bardo também guarda os arquivos da tomada por um tempo num bucket seu de armazenamento de objetos (TOS). O cartão tem cinco campos: a chave de API do ModelArk, um ID de chave de acesso e o segredo dele, o bucket e a região.

1. Abra o [console da BytePlus](https://console.byteplus.com/) e entre, ou crie uma conta. Conclua a verificação da conta quando o console pedir: os modelos Seedance do ModelArk e o TOS exigem uma conta verificada.
2. Adicione saldo na [visão geral de cobrança](https://console.byteplus.com/finance/overview) (Billing overview). O ModelArk só libera o Seedance 2.5 para uma conta verificada com pelo menos US$ 30 em Credits, um AI Savings Plan de US$ 30 ou mais, ou um pacote de recursos do modelo. Cada tomada é cobrada em tokens: cerca de US$ 3,70 por um rascunho de 30 segundos e US$ 20 pela versão final dele.
3. Abra a [ativação de modelos](https://ai.byteplus.com/ark/region:ap-southeast-1/openManagement?LLM=%7B%7D&advancedActiveKey=model) (Model activation) do ModelArk e ative o **Dreamina Seedance 2.5**. Fique na região `ap-southeast-1`, a que aparece no endereço da página.
4. Abra o [gerenciamento de chaves de API](https://ai.byteplus.com/ark/region:ap-southeast-1/apiKey?apikey=%7B%7D) (API key management), crie uma chave de API e dê um nome a ela (por exemplo `Bardo`). Copie a chave.
5. Crie o bucket. Abra o [console do TOS](https://console.byteplus.com/tos/bucket), escolha **Buckets** à esquerda e depois **Create bucket**. Defina:
   - **Name**: qualquer nome livre, como `bardo-takes-` seguido de algumas letras suas (letras minúsculas, dígitos e hífens);
   - **Region**: `ap-southeast-1`, a mesma do ModelArk;
   - **Versioning**: desligado;
   - **Bucket policy**: **Private**.

   Deixe o resto como está e escolha **Create**.
6. Adicione uma regra de ciclo de vida, para que nada fique no bucket se o Bardo não conseguir apagar (por exemplo, com o computador desligado por semanas). No bucket, escolha **Basic configurations › Lifecycle**, depois **Create rule**, e defina:
   - **Rule name**: `bardo-takes`;
   - **Status**: **Enable**;
   - **Impact Scope**: **Specify prefix**, `bardo/takes/`;
   - **Effective policy**: **Last modified time**;
   - **Latest file version expiration policy**: ligada, **Specify days**, `8`, e apagar.

   Escolha **Create** e depois **OK**. O Bardo apaga sozinho os arquivos de cada tomada assim que não precisa mais deles; esta regra é só a garantia.
7. Crie uma chave de acesso que só alcança esse bucket. Abra o **IAM** pelo seu nome de usuário no alto do console, escolha **Permission policy** e depois **Create policy**, dê o nome `bardo-takes` e cole isto como conteúdo, com o nome do seu bucket no lugar de `BUCKET`:

   ```json
   {
     "Statement": [
       {
         "Effect": "Allow",
         "Action": ["tos:PutObject", "tos:GetObject", "tos:DeleteObject"],
         "Resource": ["trn:tos:::BUCKET/bardo/takes/*"]
       }
     ]
   }
   ```

   Escolha **Submit**. Depois escolha **User management › User** e **Create new user**, dê o nome `bardo`, ligue **Programmatic access** e deixe **Console access** desligado, escolha **Next**, adicione a política `bardo-takes`, escolha **Next** e **Submit**. Na aba **Access key** do usuário, escolha **Create access key** e copie o ID da chave de acesso e a chave de acesso secreta. O segredo aparece só desta vez.
8. Preencha o cartão **BytePlus ModelArk** em [Configurações › Chaves de API](bardo:go/settings/keys): a chave de API, o ID da chave de acesso, a chave de acesso secreta, o nome do bucket e a região (`ap-southeast-1`, a não ser que você tenha criado o bucket em outra). Escolha **Salvar chave** e depois **Testar chave**. O teste pede ao ModelArk uma tarefa que não pode existir e grava, lê e apaga um arquivo pequeno em `bardo/takes/`, o que custa quase nada. Para trocar qualquer um dos cinco depois, preencha os cinco de novo.

O Seedance 2.5 recusa rostos reais no que recebe, então uma foto sua vai pela biblioteca de assets do ModelArk, pelo id dela na aparência da persona (Personas › A aparência). Uma aparência desenhada pode ir como imagem, ou também por um id de asset. No [Model Playground](https://ai.byteplus.com/ark/region:ap-southeast-1/experience/gen_video?model=dreamina-seedance-2-5-260628) do ModelArk:

- **A sua semelhança**: escolha a aba **My assets** abaixo da caixa de entrada, depois **Real-human** e **Add real-human assets**, e siga os passos: você se verifica pelo celular e envia fotos nítidas, de frente. Aceite o asset quando ele aparecer como autorizado, abra-o e copie o ID do asset quando o status for **Active**.
- **Um personagem pronto**: escolha a aba **Digital Character Library**, escolha um personagem e copie o ID do asset dele. Ele troca o rosto de uma aparência desenhada pelo personagem do ModelArk.
- **Um retrato virtual**: se o ModelArk oferecer um retrato virtual privado em **My assets**, feito a partir da sua aparência desenhada, copie o ID do asset do mesmo jeito. O ModelArk exige que ele não se pareça com nenhuma pessoa real.

<a id="claude"></a>
## Claude

1. Abra o [Claude Console](https://platform.claude.com/) e entre, ou crie uma conta.
2. Adicione créditos: abra [Settings › Billing](https://platform.claude.com/settings/billing) e escolha **Buy credits**. A API não responde sem saldo. Na mesma página dá para ligar a recarga automática (**Auto-reload**).
3. Abra [Settings › API keys](https://platform.claude.com/settings/keys) e escolha **Create key**. Dê um nome (por exemplo `Bardo`), escolha a validade e deixe **Linked account** como você mesmo.
4. Copie a chave. Ela começa com `sk-ant-` e aparece só desta vez.
5. Cole no cartão **Claude** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="elevenlabs"></a>
## ElevenLabs

1. Abra a [ElevenLabs](https://elevenlabs.io/app/sign-up) e entre, ou crie uma conta. A narração e a música gastam os créditos do seu plano. A música pela API exige um plano pago (Starter ou acima); os planos pagos também trazem a licença comercial da música.
2. Abra [Developers › API Keys](https://elevenlabs.io/app/developers/api-keys) e escolha **Create key**. Dê um nome (por exemplo `Bardo`).
3. Deixe **Restrict Key** ligado, dê à chave acesso aos quatro recursos que o Bardo usa e deixe o resto em **No Access** (você também pode pôr um limite de créditos na chave):
   - **Text to Speech**: Access (a narração);
   - **Voices**: Read (as suas vozes, clones incluídos);
   - **Forced Alignment**: Access (o tempo de cada palavra para as legendas);
   - **Music**: Access (a música do vídeo, se você gerar no Bardo).
4. Escolha **Create key** e copie a chave. Ela aparece só desta vez.
5. Cole no cartão **ElevenLabs** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**. Uma chave sem um desses recursos falha com uma mensagem que diz qual falta.

<a id="gemini"></a>
## Gemini

1. Abra o [Google AI Studio](https://aistudio.google.com/) com a sua conta do Google e aceite os termos. Uma conta nova ganha um projeto do Google Cloud criado para ela; se você já usa o Google Cloud, traga um projeto em [Projects](https://aistudio.google.com/projects) com **Import projects**.
2. Configure o faturamento. Os modelos de imagem (Nano Banana) e de vídeo (Veo) do Google não têm nível gratuito, então o projeto precisa de uma conta de faturamento. Em [API Keys](https://aistudio.google.com/api-keys) ou [Projects](https://aistudio.google.com/projects), escolha **Set up billing** ao lado do projeto e siga os passos; uma conta pré-paga precisa de saldo, que você gerencia em [Billing](https://aistudio.google.com/billing). Quando o saldo pré-pago chega a zero, o Google recusa as chamadas até você recarregar.
3. Em [API Keys](https://aistudio.google.com/api-keys), escolha **Create API key** e o projeto.
4. Copie a chave. Dependendo de quando foi criada, ela começa com `AIza` ou com `AQ.`; copie a chave inteira.
5. Cole no cartão **Gemini** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="higgsfield"></a>
## Higgsfield

Você só precisa desta chave para clipes de vídeo dos modelos da Higgsfield.

1. Abra o [console da Higgsfield](https://open.higgsfield.ai/auth/sign-up) e crie uma conta, ou entre.
2. Adicione créditos em [Credits](https://open.higgsfield.ai/credits). Cada geração é paga com esse saldo; sem créditos, a Higgsfield recusa a chamada.
3. Abra [API keys](https://open.higgsfield.ai/api-keys) e crie uma chave. A Higgsfield dá duas partes para cada chave: um **ID da chave** (key ID) e uma **chave secreta** (secret).
4. Copie as duas. O Bardo recebe as duas num campo só, unidas por dois-pontos: `KEY_ID:KEY_SECRET` (o ID, um `:` e a chave secreta, sem espaços).
5. Cole no cartão **Higgsfield** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="kling"></a>
## Kling

Você só precisa desta chave para clipes de vídeo ou clipes interpretados direto do Kling. Os modelos do Kling também são oferecidos pela Higgsfield; ir direto custa o mesmo em 1080p e menos em 720p, pago com os pacotes de recursos do próprio Kling.

1. Abra a [plataforma de API do Kling](https://kling.ai/dev) e entre. A sua conta de desenvolvedor é a mesma da sua conta web do Kling AI.
2. Compre um pacote de recursos de vídeo em [Pricing](https://kling.ai/dev/pricing?scrollTo=video). Cada clipe gasta unidades dele por segundo (1 unidade custa US$ 0,14; o Kling 3.0 em 720p gasta 0,6 unidade por segundo). Há um pacote de teste para a primeira vez. Os pacotes vencem e, sem unidades, o Kling recusa a chamada.
3. Abra [API Keys](https://kling.ai/dev/api-key) e escolha **+ Create a new API Key**. Dê um nome a ela, como Bardo, e confirme.
4. Copie a chave. Ela aparece só desta vez. Use esta chave de API, não um par **Access Key** e **Secret Key**: os endpoints novos do Kling, que são os que o Bardo chama, não aceitam o par.
5. Cole no cartão **Kling** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

<a id="typesafe"></a>
## TypeSafe (JEV)

1. Abra o [console da TypeSafe](https://console.typesafe.ai/) e entre, ou crie uma conta.
2. Abra [API keys](https://console.typesafe.ai/keys) e crie uma chave. Dê um nome (por exemplo `Bardo`).
3. Copie a chave.
4. Cole no cartão **TypeSafe (JEV)** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**.

A [documentação da TypeSafe](https://docs.typesafe.ai/introduction/quickstart) explica mais sobre as chaves e os modelos JEV.

<a id="youtube-data"></a>
## YouTube Data API

Esta é uma chave de API simples num projeto do Google Cloud, à parte do login no YouTube que o guia Conectar o YouTube configura. A API é gratuita dentro de uma cota diária de 10.000 unidades.

1. Abra o [console do Google Cloud](https://console.cloud.google.com/) e escolha um projeto, ou [crie um](https://console.cloud.google.com/projectcreate) (por exemplo `bardo`). O projeto do login do YouTube serve.
2. Ative a [YouTube Data API v3](https://console.cloud.google.com/apis/library/youtube.googleapis.com) com **Enable** (Ativar).
3. Abra [Credentials](https://console.cloud.google.com/apis/credentials) (Credenciais) e escolha **Create credentials › API key** (Criar credenciais › Chave de API).
4. Em **API restrictions**, escolha **Restrict key** e marque **YouTube Data API v3** (o Google pede uma restrição antes de criar a chave). Deixe **Authenticate API calls through a service account** desmarcado. Escolha **Create**.
5. Copie a chave de **API key created**. Ela começa com `AIza`.
6. Cole no cartão **YouTube Data API** em [Configurações › Chaves de API](bardo:go/settings/keys), escolha **Salvar chave** e depois **Testar chave**. O teste usa 1 unidade da cota do dia. Antes, uma vez só, o cartão pede que você aceite a [política de privacidade](https://fernandolisboa.github.io/bardo/pt-BR/privacy.html) do Bardo.
